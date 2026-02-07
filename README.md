# LSS - Cross-Platform File Listing Tool

A Rust-based file listing tool that works on both Unix/Linux and Windows systems.

## Cross-Platform Features

### Platform-Specific Handling

**Unix/Linux:**
- Uses standard inode numbers
- Uses device IDs from filesystem
- Displays full Unix permission strings (rwxrwxrwx)
- Cache stored in `/etc/lss/`

**Windows:**
- Uses file index as inode substitute (NTFS unique identifier)
- Uses volume serial number as device ID
- Shows simplified permissions (always "rw-rw-rw-" for display)
- Cache stored in `%LOCALAPPDATA%\lss\` (or `C:\ProgramData\lss\` as fallback)

### Key Differences from Unix-Only Version

1. **Metadata Access**: 
   - Unix: Uses `std::os::unix::fs::MetadataExt` for inode, dev, mode, uid, gid, nlink
   - Windows: Uses `std::os::windows::fs::MetadataExt` for file_index and volume_serial_number

2. **File Identifiers**:
   - Unix: inode + device ID
   - Windows: file index + volume serial number

3. **Permissions**:
   - Unix: Full rwx permission display
   - Windows: Simplified display (Windows doesn't have Unix-style permissions)

4. **Path Separators**:
   - Both `/` and `\` are supported in ignore patterns on Windows

## Building

```bash
cargo build --release
```

## Usage

```bash
# Sort by size (calculates directory sizes)
lss -s

# Sort by name
lss -n

# Sort by type
lss -t

# Reverse sort
lss -r

# Force directory size calculation
lss -ds

# Recalculate cache
lss -rc

# Verbose output
lss --verbose

# Ignore symlinks
lss --ignore-symlinks

# Set size format (By/Bi/Kb/Mb/Gb/Tb)
lss -sf=Bi

# Ignore specific files/directories
lss --ignore=".git/,.cache/,node_modules/"
```

## Notes

- On Windows, you may need administrator privileges to create the cache directory in `C:\ProgramData\lss\`
- The cache file format is the same across platforms
- Symlink handling works on both platforms (Windows supports symlinks on NTFS with appropriate permissions)
