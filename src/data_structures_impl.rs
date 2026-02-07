use crate::data_structures;
use data_structures::Cache;
use data_structures::CacheEntry;
use data_structures::ColumnWidths;
use data_structures::FileInfo;
use data_structures::FileMetadata;
use data_structures::Logger;
use data_structures::SizeFormat;
use data_structures::SizeUnit;
use data_structures::Spinner;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

// Platform-specific imports
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

impl FileMetadata {
    fn from_metadata(_metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            Self {
                mode: _metadata.mode(),
                nlink: _metadata.nlink(),
                uid: _metadata.uid(),
                gid: _metadata.gid(),
            }
        }
        
        #[cfg(windows)]
        {
            // Windows doesn't have these exact concepts, so we use defaults
            Self {
                mode: 0o644, // Default read/write permissions
                nlink: 1,
                uid: 0,
                gid: 0,
            }
        }
    }
}

impl SizeUnit {
    pub fn to_u16(&self) -> u16 {
        *self as u16
    }

    pub fn from_u16(value: u16) -> Option<Self> {
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

impl Spinner {
    pub fn new() -> Self {
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

impl Logger {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    pub fn info(&self, message: &str) {
        if self.verbose {
            println!("{}", message);
        }
    }

    pub fn warning(&self, message: &str) {
        if self.verbose {
            eprintln!("Warning: {}", message);
        }
    }

    pub fn start_loading(&self, spinner: &mut Spinner, message: &str) {
        if !self.verbose {
            print!("\r{} {} ", spinner.next(), message);
        } else {
            println!("{}", message);
        }
    }
    
    #[allow(dead_code)]
    fn update_loading(&self, spinner: &mut Spinner, message: &str) {
        if !self.verbose {
            print!("\r{} {} ", spinner.next(), message);
        }
    }

    pub fn end_loading(&self) {
        if !self.verbose {
            print!("\r");
        }
    }

    pub fn progress(&self, spinner: &mut Spinner, current: usize, total: usize, message: &str) {
        if !self.verbose {
            print!("\r{} {} ({}/{}) ", spinner.next(), message, current, total);
        } else if current % 10 == 0 || current == total {
            println!("{} ({}/{})", message, current, total);
        }
    }
}

impl FileInfo {
    pub fn new(path: &Path, name: String, ignore_symlinks: bool) -> io::Result<Self> {
        let metadata = if ignore_symlinks {
            fs::symlink_metadata(path)?
        } else {
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

        let inode = Self::get_inode_from_metadata(&metadata);

        Ok(FileInfo {
            inode,
            size: metadata.len(),
            name,
            file_type,
            metadata: FileMetadata::from_metadata(&metadata),
            is_directory,
            full_path: path.to_path_buf(),
        })
    }

    #[cfg(unix)]
    fn get_inode_from_metadata(metadata: &fs::Metadata) -> u64 {
        metadata.ino()
    }

    #[cfg(windows)]
    fn get_inode_from_metadata(metadata: &fs::Metadata) -> u64 {
        // On Windows, we'll use creation time + file size as a pseudo-inode
        // This isn't perfect but works for most cases on stable Rust
        use std::time::UNIX_EPOCH;
        
        let created = metadata.created().unwrap_or(UNIX_EPOCH);
        let created_secs = created.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Combine creation time and size for a reasonably unique identifier
        created_secs.wrapping_mul(31).wrapping_add(metadata.len())
    }

    pub fn calculate_directory_size(
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
                        let entry_inode = Self::get_inode_from_metadata(&metadata);
                        let entry_device = Self::get_device_id_from_metadata(&metadata);
                        
                        if entry_inode == self.inode && entry_device == self.get_device_id() {
                            continue;
                        }

                        let subdir_key = (entry_inode, entry_device);
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
    
    #[allow(dead_code)]
    fn times_equal(&self, _other: &SystemTime) -> bool {
        true
    }
    
    #[allow(dead_code)]
    fn system_time_to_secs(&self, _time: &SystemTime) -> u64 {
        1
    }

    pub fn should_ignore(path: &Path, ignore_patterns: &[String]) -> bool {
        for pattern in ignore_patterns {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                continue;
            }

            if pattern.ends_with('/') || pattern.ends_with('\\') {
                let dir_pattern = pattern.trim_end_matches('/').trim_end_matches('\\');
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

    #[cfg(unix)]
    fn get_device_id(&self) -> u64 {
        if let Ok(metadata) = fs::metadata(&self.full_path) {
            metadata.dev()
        } else {
            0
        }
    }

    #[cfg(windows)]
    fn get_device_id(&self) -> u64 {
        // Use a hash of the drive/volume path as device ID
        use sha2::{Digest, Sha256};
        
        if let Some(prefix) = self.full_path.components().next() {
            let mut hasher = Sha256::new();
            hasher.update(format!("{:?}", prefix).as_bytes());
            let result = hasher.finalize();
            u64::from_le_bytes([
                result[0], result[1], result[2], result[3],
                result[4], result[5], result[6], result[7],
            ])
        } else {
            0
        }
    }

    #[cfg(unix)]
    fn get_device_id_from_metadata(metadata: &fs::Metadata) -> u64 {
        metadata.dev()
    }

    #[cfg(windows)]
    fn get_device_id_from_metadata(_metadata: &fs::Metadata) -> u64 {
        // For metadata without path context, return a constant
        // This is less than ideal but works with stable Rust
        1
    }

    fn format_permissions(&self) -> String {
        #[cfg(unix)]
        {
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

        #[cfg(windows)]
        {
            // Windows simplified permissions display
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

            // Windows files are generally readable
            permissions.push_str("rw-rw-rw-");
            permissions
        }
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

impl ColumnWidths {
    pub fn new() -> Self {
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

    pub fn calculate_from_files(&mut self, files: &[FileInfo], size_format: &SizeFormat) {
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

    pub fn display_header(&self) {
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

    pub fn display_file(&self, file: &FileInfo, size_format: &SizeFormat) {
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
