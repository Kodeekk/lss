use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
mod data_structures;
mod data_structures_impl;
use data_structures::{
    Cache, CacheEntry, ColumnWidths, FileInfo, Logger, SizeFormat, SizeUnit, Spinner,
};

const CACHE_DIR: &str = "/etc/lss";
const CACHE_FILE: &str = "global_cache.bin";

fn parse_size_format(format_str: &str) -> Result<SizeFormat, String> {
    match format_str.to_lowercase().as_str() {
        "by" | "bytes" => Ok(SizeFormat::Bytes),
        "bi" | "binary" => Ok(SizeFormat::Binary),
        "kb" | "mb" | "gb" | "tb" | "decimal" => Ok(SizeFormat::Decimal),
        _ => Err(format!("Unknown size format: {}", format_str)),
    }
}

fn parse_ignore_patterns(ignore_str: &str) -> Vec<String> {
    ignore_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn ensure_cache_dir() -> io::Result<()> {
    let cache_dir = Path::new(CACHE_DIR);
    if !cache_dir.exists() {
        fs::create_dir_all(cache_dir)?;
    }
    Ok(())
}

fn get_cache_path() -> PathBuf {
    Path::new(CACHE_DIR).join(CACHE_FILE)
}

fn save_cache(cache: &Cache, logger: &Logger) -> io::Result<()> {
    ensure_cache_dir()?;
    let cache_path = get_cache_path();

    let mut file = File::create(&cache_path)?;

    for (key, entry) in cache {
        let key_bytes = key.as_bytes();
        if key_bytes.len() > u16::MAX as usize {
            logger.warning(&format!("Cache key too long, skipping: {}", key));
            continue;
        }

        file.write_all(&(key_bytes.len() as u16).to_le_bytes())?;
        file.write_all(key_bytes)?;

        let inode_bytes = entry.inode.to_le_bytes();
        file.write_all(&inode_bytes)?;
        file.write_all(&[0u8; 2])?;

        let size_bytes = entry.size.to_le_bytes();
        file.write_all(&size_bytes)?;

        let unit_bytes = entry.size_unit.to_u16().to_le_bytes();
        file.write_all(&unit_bytes)?;

        let device_bytes = entry.device_id.to_le_bytes();
        file.write_all(&device_bytes)?;
    }

    logger.info(&format!(
        "Cache saved to: {} ({} entries)",
        cache_path.display(),
        cache.len()
    ));
    Ok(())
}

fn load_cache(logger: &Logger) -> io::Result<Cache> {
    let cache_path = get_cache_path();
    let mut cache = HashMap::new();

    if !cache_path.exists() {
        logger.info(&format!("No cache file found at: {}", cache_path.display()));
        return Ok(cache);
    }

    let mut file = File::open(&cache_path)?;
    let metadata = fs::metadata(&cache_path)?;

    if metadata.len() == 0 {
        logger.info("Cache file is empty");
        return Ok(cache);
    }

    if metadata.len() > 100 * 1024 * 1024 {
        logger.warning(&format!(
            "Cache file too large ({} bytes), using empty cache",
            metadata.len()
        ));
        return Ok(cache);
    }

    let mut corrupted_entries = 0;

    loop {
        let mut key_len_buf = [0u8; 2];
        if file.read_exact(&mut key_len_buf).is_err() {
            break;
        }
        let key_len = u16::from_le_bytes(key_len_buf) as usize;

        if key_len == 0 || key_len > 4096 {
            corrupted_entries += 1;
            break;
        }

        let mut key_buf = vec![0u8; key_len];
        if file.read_exact(&mut key_buf).is_err() {
            corrupted_entries += 1;
            break;
        }
        let key = match String::from_utf8(key_buf) {
            Ok(k) => k,
            Err(_) => {
                corrupted_entries += 1;
                continue;
            }
        };

        let mut inode_buf = [0u8; 10];
        if file.read_exact(&mut inode_buf).is_err() {
            corrupted_entries += 1;
            break;
        }
        let inode = u64::from_le_bytes([
            inode_buf[0],
            inode_buf[1],
            inode_buf[2],
            inode_buf[3],
            inode_buf[4],
            inode_buf[5],
            inode_buf[6],
            inode_buf[7],
        ]);

        let mut size_buf = [0u8; 8];
        if file.read_exact(&mut size_buf).is_err() {
            corrupted_entries += 1;
            break;
        }
        let size = u64::from_le_bytes(size_buf);

        let mut unit_buf = [0u8; 2];
        if file.read_exact(&mut unit_buf).is_err() {
            corrupted_entries += 1;
            break;
        }
        let unit_value = u16::from_le_bytes(unit_buf);
        let size_unit = SizeUnit::from_u16(unit_value).unwrap_or(SizeUnit::Bytes);

        let mut device_buf = [0u8; 8];
        if file.read_exact(&mut device_buf).is_err() {
            corrupted_entries += 1;
            break;
        }
        let device_id = u64::from_le_bytes(device_buf);

        cache.insert(
            key,
            CacheEntry {
                size,
                inode,
                device_id,
                size_unit,
            },
        );
    }

    if corrupted_entries > 0 {
        logger.warning(&format!(
            "{} corrupted entries in cache file",
            corrupted_entries
        ));
    }

    logger.info(&format!("Cache loaded: {} entries", cache.len()));
    Ok(cache)
}

//#[unsafe(export_name = "MAINTODBG")]
fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    let mut sort_mode = "s";
    let mut reverse = false;
    let mut size_format = SizeFormat::Decimal;
    let mut calculate_dir_sizes = false;
    let mut recalculate_cache = false;
    let mut ignore_patterns: Vec<String> = Vec::new();
    let mut verbose = false;
    let mut ignore_symlinks = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-s" => {
                sort_mode = "s";
                calculate_dir_sizes = true;
            }
            "-n" => sort_mode = "n",
            "-t" => sort_mode = "t",
            "-r" => reverse = true,
            "-ds" => calculate_dir_sizes = true,
            "-rc" => recalculate_cache = true,
            "--verbose" => verbose = true,
            "--ignore-symlinks" => ignore_symlinks = true,
            arg if arg.starts_with("-sf=") => {
                let format_str = &arg[4..];
                match parse_size_format(format_str) {
                    Ok(fmt) => size_format = fmt,
                    Err(e) => {
                        eprintln!("{}", e);
                        eprintln!(
                            "Available size formats: By (bytes), Bi (binary - KiB/MiB/GiB/TiB), Kb/Mb/Gb/Tb (decimal - KB/MB/GB/TB)"
                        );
                        return Ok(());
                    }
                }
            }
            arg if arg.starts_with("--ignore=") => {
                let ignore_str = &arg[9..];
                ignore_patterns = parse_ignore_patterns(ignore_str);
                if verbose {
                    println!("Ignore patterns: {:?}", ignore_patterns);
                }
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                eprintln!(
                    "Usage: {} [-s|-n|-t] [-r] [-ds] [-rc] [--verbose] [--ignore-symlinks] [-sf=FORMAT] [--ignore=PATTERNS]",
                    args[0]
                );
                eprintln!("Size formats: By, Bi, Kb, Mb, Gb, Tb");
                eprintln!("-ds: Force directory size calculation (auto-enabled for size sorting)");
                eprintln!(
                    "-rc: Recalculate cache (ignore existing cache and recalculate all sizes)"
                );
                eprintln!("--verbose: Enable verbose output with progress details");
                eprintln!("--ignore-symlinks: Ignore symlinks when calculating directory sizes");
                eprintln!("--ignore: Comma-separated list of files/directories to ignore");
                eprintln!(
                    "          Example: --ignore=\".config/, myfile, mydir/, dir3/innerfile\""
                );
                return Ok(());
            }
        }
        i += 1;
    }

    let logger = Logger::new(verbose);
    let mut spinner = Spinner::new();
    let mut cache = load_cache(&logger)?;

    if verbose && ignore_symlinks {
        println!("Ignoring symlinks in directory size calculations");
    }

    let current_dir = Path::new(".");
    let mut files = Vec::new();

    logger.start_loading(&mut spinner, "Scanning directory...");
    let entries: Vec<_> = fs::read_dir(current_dir)?.collect();
    let total_entries = entries.len();

    for (index, entry) in entries.into_iter().enumerate() {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        logger.progress(&mut spinner, index + 1, total_entries, "Scanning directory");

        if FileInfo::should_ignore(&path, &ignore_patterns) {
            if verbose {
                println!("Ignoring: {}\t{}", name, path.display());
            }
            continue;
        }

        if verbose {
            println!("Loading entry: {}\t{}", name, path.display());
        }

        if let Ok(mut file_info) = FileInfo::new(&path, name, ignore_symlinks) {
            if calculate_dir_sizes && file_info.is_directory {
                logger.start_loading(
                    &mut spinner,
                    &format!("Calculating size for: {}", file_info.name),
                );
                let mut visited_inodes = HashSet::new();
                if let Err(e) = file_info.calculate_directory_size(
                    &mut cache,
                    recalculate_cache,
                    &mut visited_inodes,
                    &logger,
                    ignore_symlinks,
                ) {
                    logger.warning(&format!(
                        "Could not calculate size for directory '{}': {}",
                        file_info.name, e
                    ));
                }
                logger.end_loading();
            }
            files.push(file_info);
        }
    }
    logger.end_loading();

    if calculate_dir_sizes {
        save_cache(&cache, &logger)?;
    }

    match sort_mode {
        "s" => files.sort_by(|a, b| a.size.cmp(&b.size)),
        "n" => files.sort_by(|a, b| a.name.cmp(&b.name)),
        "t" => files.sort_by(|a, b| a.file_type.cmp(&b.file_type)),
        _ => files.sort_by(|a, b| a.size.cmp(&b.size)),
    }

    if reverse {
        files.reverse();
    }

    let mut col_widths = ColumnWidths::new();
    col_widths.calculate_from_files(&files, &size_format);
    col_widths.display_header();

    for file in &files {
        col_widths.display_file(&file, &size_format);
    }

    println!();
    println!("Total items: {}", files.len());
    if calculate_dir_sizes {
        if recalculate_cache {
            println!("Note: All directory sizes were recalculated and global cache was updated");
        } else {
            println!("Note: Directory sizes loaded from global cache where available");
        }
    }
    if ignore_symlinks {
        println!("Note: Symlinks were ignored in directory size calculations");
    }
    if !ignore_patterns.is_empty() {
        println!("Ignored patterns: {:?}", ignore_patterns);
    }
    println!("Global cache location: {}", get_cache_path().display());
    Ok(())
}
