Optional bundling location for zapret binaries.

If you drop a working copy of Flowseal/zapret-discord-youtube here
(so that `bin/winws.exe` exists under this folder), FN will use it directly
instead of downloading the latest release from GitHub on first launch.

Expected layout (mirrors the upstream release archive):

  resources/zapret/
    bin/
      winws.exe
      WinDivert.dll
      WinDivert64.sys
    lists/            (hostlists + ipset-*.txt)
    *.bat             (list-general strategy files, optional)

Leaving this folder empty is fine — FN downloads and extracts the release
into %APPDATA%\FN\zapret on first run.
