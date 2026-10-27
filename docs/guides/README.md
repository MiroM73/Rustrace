# RusTrace guides

Step-by-step guides for real troubleshooting situations: which logs to open, what to set in
RusTrace, what to look for and what to run from the command line. Every guide has annotated
screenshots; the numbers in the pictures match the numbered points in the text.

| Guide | Situation |
|---|---|
| [An application deployment failed](failed-app-install.md) | A required ConfigMgr application does not install (exit code 1603 / `0x80070643`): log set, errors only, error code explanation, Δ time, bookmarks, filter by application, MSI log, export for a ticket. |

The screenshots are made with invented sample logs; `python scripts/gen_guide_logs.py` creates the
same logs in `testlogs\guides\` so you can try every step yourself.
