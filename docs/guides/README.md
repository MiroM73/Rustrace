# RusTrace guides

Step-by-step guides for real troubleshooting situations: which logs to open, what to set in
RusTrace, what to look for and what to run from the command line. Every guide has annotated
screenshots; the numbers in the pictures match the numbered points in the text.

| Guide | Situation |
|---|---|
| [An application deployment failed](failed-app-install.md) | A required ConfigMgr application does not install (exit code 1603 / `0x80070643`): log set, errors only, error code explanation, Δ time, bookmarks, filter by application, MSI log, export for a ticket. |
| [Software updates do not install](software-updates.md) | The monthly updates fail on a remote client (`0x80070070`, scan error `0x80244022`): log set on a remote computer, CMTrace and CBS.log merged, column filters, time range around the deadline, highlight rule for one update, raw line, export of the rows shown. |
| [A new ConfigMgr client does not register](client-install.md) | ccmsetup succeeded, but the client cannot reach its management point (`0x80072ee7`): rotated logs, rows without a date, harmless MSI notes, regex milestones, time reference, bookmarks list, error code lookup. |
| [The management point / distribution point is not healthy](iis-management-point.md) | The IIS log of the site server (HTTP 500 after 30 s, 503.2, 401.1 + 1326, 404.3): own log definition with the status columns first, HTTP / IIS status explanations, column filter `!=2`, regex search in extra columns. |
| [The ConfigMgr client installation fails](client-msi-1603.md) | ccmsetup.log and the verbose client.msi.log (1618, then 1603 in a custom action with a WMI error): failed action in the details pane, misleading keyword errors in MSI logs, MSI highlight rules (importable JSON), the first `Return value 3`, copying the evidence, the fix. |

**In RusTrace**: Help → Guides lists every guide (*All guides…* opens this page), Help → About
links here, and the log sets *Application deployment*, *Software updates* and *Client installation*
have a 📖 button for their guide in File → Open log set….

The screenshots are made with invented sample logs; `python scripts/gen_guide_logs.py` creates the
same logs in `testlogs\guides\` so you can try every step yourself.
