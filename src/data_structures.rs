use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SizeFormat {
    Bytes,
    Binary,
    Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub inode: u64,
    pub size: u64,
    pub name: String,
    // created: SystemTime, //i dont give a shit that half of functionality is commented, i dont want to fuck with this time-things anymore
    // modified: SystemTime,
    pub file_type: String,
    pub metadata: FileMetadata,
    pub is_directory: bool,
    pub full_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeUnit {
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

#[derive(Debug)]
pub struct CacheEntry {
    pub size: u64,
    pub inode: u64,
    pub device_id: u64,
    pub size_unit: SizeUnit,
}

pub type Cache = HashMap<String, CacheEntry>;

pub struct Spinner {
    pub frames: Vec<char>,
    pub current: usize,
}

pub struct Logger {
    pub verbose: bool,
}

pub struct ColumnWidths {
    pub inode: usize,
    pub permissions: usize,
    pub links: usize,
    pub uid: usize,
    pub gid: usize,
    pub size: usize,
    pub time: usize,
    pub file_type: usize,
    pub name: usize,
}
