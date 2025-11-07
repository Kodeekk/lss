# lss

lists all the files sorted in current directory

some day i just dreamed about 'ls' command or some file manager being able to sort files by size
fck it, i wrote it myself

usage it quite simple:
  sorting modes:
    -s (sort by entry size)
    -n (sort by entry name)
    -t (sort by entry type)
    if mode flag is not passed, default sorting mode will be as -s
    -r (reverse the order)
  
  --verbose (prints log and other useless sht)
  --ignore-symlinks (just makes it ignore symlinks)
  -sf (stands for "Size Format")
  Available size formats: By (bytes), Bi (binary - KiB/MiB/GiB/TiB), Kb/Mb/Gb/Tb (decimal - KB/MB/GB/TB)
  examples of -sf:
    -sf=GB
    -sf=KiB
  -rc (recalculates file and directory sizes and rewrites the cache file)
  --ignore (allows you to pass entries to be ignored separated by comma)
  examples of --ignore:
    --ignore=".local/, foo.txt bar"
    --ignore=".local, .local/share/, ../abc.rcf"

for the first run, its recommended to use sudo (to create "/etc/lss/" and "/etc/lss/global_cache.bin")
at this point i can say that this is 100% best utility i ever created
dude it even has spinner animation
