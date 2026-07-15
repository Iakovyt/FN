The compiled TgWsProxy_headless.exe is generated here before a production
build and is intentionally excluded from Git.

TGWS 1.8.1 source code is stored in source/proxy. Run:

  npm run build:tgws

The build script creates the headless executable with PyInstaller. FN launches
it without a separate tray icon and passes --host, --port and --secret.
