# lss
### ls but sorted
lists all the files sorted in current directory

one day i was just dreamin about `ls` command or some file manager being able to sort files by size
fck it, i wrote it myself

usage it quite simple:

**sorting modes:**
 - `-s` - sort by entry sizes
 - `-n` - sorts by entry names
 - `-t` - sorts by entry types
 - `-r` - reverses the order
 *if mode flag is not passed, default sorting mode will be as* `-s`

**other useful flags:**

 - `--verbose` - prints log and other useless sht
 - `--ignore-symlinks` - just makes it ignore symlinks
 - `-rc` - recalculates file and directory sizes and rewrites the cache file
 - `--ignore` - allows you to pass entries to be ignored separated by comma
 - `-sf` - stands for Size Format

Available size formats:
- `By` (bytes)
- `Bi` (binary)
- `Kb`/`Mb`/`Gb`/`Tb` (decimal)

Examples of `-sf`:
-    `-sf=GB`
-    `-sf=KiB`

  Examples of --ignore:
-    `--ignore=".local/, foo.txt bar"`
-    `--ignore=".local, .local/share/, ../abc.rcf"`

for the first run, its recommended to use sudo to create */etc/lss/* and */etc/lss/global_cache.bin*
at this point i can say that this is 100% best utility i ever created
dude it does even have spinner animation
