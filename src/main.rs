use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::io::Write;
use std::io::{self};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum SizeFormat {
    Bytes,
    Binary,
    Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileInfo {
    inode: u64,
    size: u64,
    name: String,
    // created: SystemTime, //i dont give a shit that half of functionality is commented, i dont want to fuck with this time-things anymore
    // modified: SystemTime,
    file_type: String,
    metadata: FileMetadata,
    is_directory: bool,
    full_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileMetadata {
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
}

impl FileMetadata {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            mode: metadata.mode(),
            nlink: metadata.nlink(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SizeUnit {
    Bytes = 0x0001,
    Kilobytes = 0x0002,
    Megabytes = 0x0003,
    Gigabytes = 0x0004,
    Terabytes = 0x0005,
    Kibibytes = 0x0101,
    Mebibytes = 0x0102,
    Gibibytes = 0x0103,
    Tebibytes = 0x0104,
}

impl SizeUnit {
    fn to_u16(&self) -> u16 {
        *self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0001 => Some(SizeUnit::Bytes),
            0x0002 => Some(SizeUnit::Kilobytes),
            0x0003 => Some(SizeUnit::Megabytes),
            0x0004 => Some(SizeUnit::Gigabytes),
            0x0005 => Some(SizeUnit::Terabytes),
            0x0101 => Some(SizeUnit::Kibibytes),
            0x0102 => Some(SizeUnit::Mebibytes),
            0x0103 => Some(SizeUnit::Gibibytes),
            0x0104 => Some(SizeUnit::Tebibytes),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct CacheEntry {
    size: u64,
    inode: u64,
    device_id: u64,
    size_unit: SizeUnit,
}

type Cache = HashMap<String, CacheEntry>;

const CACHE_DIR: &str = "/etc/lss";
const CACHE_FILE: &str = "global_cache.bin";

struct Spinner {
    frames: Vec<char>,
    current: usize,
}

impl Spinner {
    fn new() -> Self {
        Self {
            frames: vec!['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            current: 0,
        }
    }

    fn next(&mut self) -> char {
        let frame = self.frames[self.current];
        self.current = (self.current + 1) % self.frames.len();
        frame
    }
}

struct Logger {
    verbose: bool,
}

impl Logger {
    fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    fn info(&self, message: &str) {
        if self.verbose {
            println!("{}", message);
        }
    }

    fn warning(&self, message: &str) {
        if self.verbose {
            eprintln!("Warning: {}", message);
        }
    }

    fn start_loading(&self, spinner: &mut Spinner, message: &str) {
        if !self.verbose {
            print!("\r{} {} ", spinner.next(), message);
        } else {
            println!("{}", message);
        }
    }

    fn update_loading(&self, spinner: &mut Spinner, message: &str) {
        if !self.verbose {
            print!("\r{} {} ", spinner.next(), message);
        }
    }

    fn end_loading(&self) {
        if !self.verbose {
            print!("\r");
        }
    }

    fn progress(&self, spinner: &mut Spinner, current: usize, total: usize, message: &str) {
        if !self.verbose {
            print!("\r{} {} ({}/{}) ", spinner.next(), message, current, total);
        } else if current % 10 == 0 || current == total {
            // Only print every 10 items in verbose mode to avoid spam
            println!("{} ({}/{})", message, current, total);
        }
    }
}

impl FileInfo {
    fn new(path: &Path, name: String, ignore_symlinks: bool) -> io::Result<Self> {
        //already kinda forgetting how that works
        let metadata = if ignore_symlinks {
            // Use symlink_metadata to get info about the symlink itself without following it
            fs::symlink_metadata(path)?
        } else {
            // Use metadata to follow symlinks and get info about the target
            fs::metadata(path)?
        };

        let is_directory = metadata.is_dir();

        let file_type = if is_directory {
            "directory".to_string()
        } else if metadata.file_type().is_symlink() {
            "symlink".to_string()
        } else if metadata.is_file() {
            "file".to_string()
        } else {
            "other".to_string()
        };

        Ok(FileInfo {
            inode: metadata.ino(),
            size: metadata.len(),
            name,
            file_type,
            metadata: FileMetadata::from_metadata(&metadata),
            is_directory,
            full_path: path.to_path_buf(),
        })
    }

    fn calculate_directory_size(
        &mut self,
        cache: &mut Cache,
        recalculate: bool,
        visited_inodes: &mut HashSet<(u64, u64)>,
        logger: &Logger,
        ignore_symlinks: bool,
    ) -> io::Result<u64> {
        if !self.is_directory {
            return Ok(self.size);
        }

        let current_key = (self.inode, self.get_device_id());
        if visited_inodes.contains(&current_key) {
            logger.warning(&format!(
                "Detected directory cycle at {}",
                self.full_path.display()
            ));
            return Ok(0);
        }
        visited_inodes.insert(current_key.clone());

        let cache_key = self.get_cache_key();

        if !recalculate {
            if let Some(entry) = cache.get(&cache_key) {
                if self.get_device_id() == entry.device_id {
                    self.size = match entry.size_unit {
                        SizeUnit::Bytes => entry.size,
                        SizeUnit::Kilobytes => entry.size * 1000,
                        SizeUnit::Megabytes => entry.size * 1_000_000,
                        SizeUnit::Gigabytes => entry.size * 1_000_000_000,
                        SizeUnit::Terabytes => entry.size * 1_000_000_000_000,
                        SizeUnit::Kibibytes => entry.size * 1024,
                        SizeUnit::Mebibytes => entry.size * 1_048_576,
                        SizeUnit::Gibibytes => entry.size * 1_073_741_824,
                        SizeUnit::Tebibytes => entry.size * 1_099_511_627_776,
                    };
                    visited_inodes.remove(&current_key);
                    return Ok(self.size);
                }
            }
        }

        let mut total_size = 0u64;
        let mut entry_count = 0;
        let mut error_count = 0;

        let entries = match fs::read_dir(&self.full_path) {
            Ok(entries) => entries,
            Err(e) => {
                logger.warning(&format!(
                    "Could not read directory '{}': {}",
                    self.full_path.display(),
                    e
                ));
                visited_inodes.remove(&current_key);
                return Ok(0);
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    error_count += 1;
                    if error_count <= 5 {
                        logger.warning(&format!(
                            "Could not read entry in '{}': {}",
                            self.full_path.display(),
                            e
                        ));
                    }
                    continue;
                }
            };

            let path = entry.path();
            entry_count += 1;

            let metadata_result = if ignore_symlinks {
                fs::symlink_metadata(&path)
            } else {
                fs::metadata(&path)
            };

            match metadata_result {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        if metadata.ino() == self.inode && metadata.dev() == self.get_device_id() {
                            continue;
                        }

                        let subdir_key = (metadata.ino(), metadata.dev());
                        if visited_inodes.contains(&subdir_key) {
                            continue;
                        }

                        let name = entry.file_name().to_string_lossy().to_string();
                        match FileInfo::new(&path, name, ignore_symlinks) {
                            Ok(mut subdir_info) => {
                                match subdir_info.calculate_directory_size(
                                    cache,
                                    recalculate,
                                    visited_inodes,
                                    logger,
                                    ignore_symlinks,
                                ) {
                                    Ok(subdir_size) => {
                                        total_size = total_size.saturating_add(subdir_size);
                                    }
                                    Err(e) => {
                                        error_count += 1;
                                        if error_count <= 5 {
                                            logger.warning(&format!(
                                                "Could not calculate size for '{}': {}",
                                                path.display(),
                                                e
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error_count += 1;
                                if error_count <= 5 {
                                    logger.warning(&format!(
                                        "Could not create FileInfo for '{}': {}",
                                        path.display(),
                                        e
                                    ));
                                }
                            }
                        }
                    } else {
                        if ignore_symlinks && metadata.file_type().is_symlink() {
                            continue;
                        }
                        total_size = total_size.saturating_add(metadata.len());
                    }
                }
                Err(e) => {
                    error_count += 1;
                    if error_count <= 5 {
                        logger.warning(&format!(
                            "Could not get metadata for '{}': {}",
                            path.display(),
                            e
                        ));
                    }
                }
            }
        }

        if error_count > 5 {
            logger.warning(&format!(
                "{} additional errors in '{}'",
                error_count - 5,
                self.full_path.display()
            ));
        }

        self.size = total_size;

        cache.insert(
            cache_key,
            CacheEntry {
                size: total_size,
                inode: self.inode,
                device_id: self.get_device_id(),
                size_unit: SizeUnit::Bytes,
            },
        );

        visited_inodes.remove(&current_key);

        logger.info(&format!(
            "Directory '{}': {} entries, {} errors, total size: {} bytes",
            self.name, entry_count, error_count, total_size
        ));

        Ok(total_size)
    }

    fn times_equal(&self, _other: &SystemTime) -> bool {
        true
    }

    fn system_time_to_secs(&self, _time: &SystemTime) -> u64 {
        1
    }

    fn should_ignore(path: &Path, ignore_patterns: &[String]) -> bool {
        for pattern in ignore_patterns {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                continue;
            }

            if pattern.ends_with('/') {
                let dir_pattern = pattern.trim_end_matches('/');
                if path.is_dir() {
                    if let Some(file_name) = path.file_name() {
                        if file_name.to_string_lossy() == dir_pattern {
                            return true;
                        }
                    }
                }
            } else if let Some(file_name) = path.file_name() {
                if file_name.to_string_lossy() == pattern {
                    return true;
                }
            }
        }
        false
    }

    fn get_cache_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.inode.to_string().as_bytes());
        hasher.update(self.get_device_id().to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn get_device_id(&self) -> u64 {
        if let Ok(metadata) = fs::metadata(&self.full_path) {
            metadata.dev()
        } else {
            0
        }
    }

    fn format_permissions(&self) -> String {
        let mode = self.metadata.mode;
        let mut permissions = String::with_capacity(10);

        permissions.push(if self.is_directory {
            'd'
        } else if self.file_type == "symlink" {
            'l'
        } else if self.file_type == "file" {
            '-'
        } else {
            '?'
        });

        permissions.push(if mode & 0o400 != 0 { 'r' } else { '-' });
        permissions.push(if mode & 0o200 != 0 { 'w' } else { '-' });
        permissions.push(if mode & 0o100 != 0 { 'x' } else { '-' });
        permissions.push(if mode & 0o040 != 0 { 'r' } else { '-' });
        permissions.push(if mode & 0o020 != 0 { 'w' } else { '-' });
        permissions.push(if mode & 0o010 != 0 { 'x' } else { '-' });
        permissions.push(if mode & 0o004 != 0 { 'r' } else { '-' });
        permissions.push(if mode & 0o002 != 0 { 'w' } else { '-' });
        permissions.push(if mode & 0o001 != 0 { 'x' } else { '-' });

        permissions
    }

    fn format_time(&self) -> String {
        "1".into()
    }

    fn format_size(&self, size_format: &SizeFormat) -> String {
        match size_format {
            SizeFormat::Bytes => format!("{}", self.size),
            SizeFormat::Binary => self.format_size_binary(),
            SizeFormat::Decimal => self.format_size_decimal(),
        }
    }

    fn format_size_binary(&self) -> String {
        const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
        let mut size = self.size as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} B", size as u64)
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    fn format_size_decimal(&self) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let mut size = self.size as f64;
        let mut unit_index = 0;

        while size >= 1000.0 && unit_index < UNITS.len() - 1 {
            size /= 1000.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} B", size as u64)
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    fn get_display_fields(
        &self,
        size_format: &SizeFormat,
    ) -> (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) {
        (
            format!("{}", self.inode),
            self.format_permissions(),
            format!("{}", self.metadata.nlink),
            format!("{}", self.metadata.uid),
            format!("{}", self.metadata.gid),
            self.format_size(size_format),
            self.format_time(),
            self.file_type.clone(),
        )
    }
}

struct ColumnWidths {
    inode: usize,
    permissions: usize,
    links: usize,
    uid: usize,
    gid: usize,
    size: usize,
    time: usize,
    file_type: usize,
    name: usize,
}

impl ColumnWidths {
    fn new() -> Self {
        Self {
            inode: 8,
            permissions: 10,
            links: 4,
            uid: 8,
            gid: 8,
            size: 10,
            time: 12,
            file_type: 10,
            name: 20,
        }
    }

    fn calculate_from_files(&mut self, files: &[FileInfo], size_format: &SizeFormat) {
        for file in files {
            let (inode, permissions, links, uid, gid, size, time, file_type) =
                file.get_display_fields(size_format);

            self.inode = self.inode.max(inode.len());
            self.permissions = self.permissions.max(permissions.len());
            self.links = self.links.max(links.len());
            self.uid = self.uid.max(uid.len());
            self.gid = self.gid.max(gid.len());
            self.size = self.size.max(size.len());
            self.time = self.time.max(time.len());
            self.file_type = self.file_type.max(file_type.len());
            self.name = self.name.max(file.name.len());
        }

        self.inode += 2;
        self.permissions += 2;
        self.links += 2;
        self.uid += 2;
        self.gid += 2;
        self.size += 2;
        self.time += 2;
        self.file_type += 2;
        self.name += 2;
    }

    fn display_header(&self) {
        println!(
            "{:inode$}{:permissions$}{:links$}{:uid$}{:gid$}{:size$}{:time$}{:file_type$}{:name$}",
            "Inode",
            "Permissions",
            "Links",
            "UID",
            "GID",
            "Size",
            "Modified",
            "Type",
            "Name",
            inode = self.inode,
            permissions = self.permissions,
            links = self.links,
            uid = self.uid,
            gid = self.gid,
            size = self.size,
            time = self.time,
            file_type = self.file_type,
            name = self.name,
        );

        let total_width = self.inode
            + self.permissions
            + self.links
            + self.uid
            + self.gid
            + self.size
            + self.time
            + self.file_type
            + self.name;
        println!("{}", "-".repeat(total_width));
    }

    fn display_file(&self, file: &FileInfo, size_format: &SizeFormat) {
        let (inode, permissions, links, uid, gid, size, time, file_type) =
            file.get_display_fields(size_format);

        println!(
            "{:inode$}{:permissions$}{:links$}{:uid$}{:gid$}{:size$}{:time$}{:file_type$}{:name$}",
            inode,
            permissions,
            links,
            uid,
            gid,
            size,
            time,
            file_type,
            file.name,
            inode = self.inode,
            permissions = self.permissions,
            links = self.links,
            uid = self.uid,
            gid = self.gid,
            size = self.size,
            time = self.time,
            file_type = self.file_type,
            name = self.name,
        );
    }
}

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

#[unsafe(export_name = "MAINTODBG")]
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
