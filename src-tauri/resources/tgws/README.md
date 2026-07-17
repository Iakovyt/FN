# TGWS source

This directory contains the TGWS 1.8.1 proxy source used by FN.

- `source/proxy/` - upstream proxy implementation.
- `source/LICENSE` - upstream GPL-3.0 license.
- `../../../scripts/tgws_headless.py` - FN headless entry point.
- `../../../scripts/build-tgws-headless.ps1` - reproducible Windows build.

The generated `TgWsProxy_headless.exe` is ignored by Git and is added only to
the application bundle. Build it with `npm run build:tgws`.
