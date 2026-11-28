# Log sets

Groups of logs for one troubleshooting scenario, opened together on this or a remote computer.

[← Back to README](../README.md)

A **log set** is a named group of logs for one troubleshooting scenario. **File → Open log set…**
opens a window with the list of sets (name and description) on the left: choose the set (the last
one is remembered) and the computer, and the window checks in the background which of its logs exist – ✔ *Found* with the size, the last write time
and the number of rotated older parts that are opened with it (`AppEnforce.lo_`,
`AppEnforce-20261027-101500.log`), ✖ *Not found*, 🔒 *Access denied*. Found logs are ticked; untick
the ones you do not need and press **Open** – the logs are opened as **one view merged by time**
(the same as opening several files, see [Several logs in one view](features.md#several-logs-in-one-view-sorted-by-time)). **Check again**
repeats the check (e.g. after starting the scenario on the client).

**Search**: type in the field above the list to show only the sets you are looking for. Every
word must appear in the name or the description (in any language) or in the path of one of the
logs, e.g. `deploy`, `CBS` or `ccmsetup client`; case does not matter. The set shown on the right
stays until you click another one. **✖** clears the search.

**Name and description**: say what a set is for when you [save it](#saving-the-opened-logs-as-a-set),
or change it later – choose one of **your own** sets and press **✏ Edit**. Only the name and
description change; the logs, guides and comments of the file stay, and so does its file name. In
a set with translations the text of the current language is changed. An empty description removes
it. Another of your own sets cannot have the same name. Built-in sets cannot be edited.

Built-in sets (`%CCMLOGS%` = the log folder of the ConfigMgr client, see below); *Application
deployment*, *Software updates* and *Client installation* have a 📖 button that opens their guide:

| Set | Logs |
|---|---|
| Application deployment | `%CCMLOGS%\` `AppDiscovery.log`, `AppEnforce.log`, `AppIntentEval.log`, `CAS.log`, `ContentTransferManager.log`, `DataTransferService.log` |
| Software updates | `%CCMLOGS%\` `WUAHandler.log`, `ScanAgent.log`, `UpdatesDeployment.log`, `UpdatesHandler.log`, `UpdatesStore.log`; `%WINDIR%\Logs\CBS\CBS.log` |
| Client installation | `%WINDIR%\ccmsetup\Logs\` `ccmsetup.log`, `client.msi.log`; `%CCMLOGS%\` `ClientIDManagerStartup.log`, `LocationServices.log` |
| Policy | `%CCMLOGS%\` `PolicyAgent.log`, `PolicyEvaluator.log`, `ClientLocation.log` |
| Intune / co-management | `%PROGRAMDATA%\Microsoft\IntuneManagementExtension\Logs\` `IntuneManagementExtension.log`, `AppWorkload.log`, `AppActionProcessor.log`, `AgentExecutor.log`; `%CCMLOGS%\CoManagementHandler.log` |

**Remote computer**: choose *Remote computer*, type its name or IP address (`pc01`,
`pc01.contoso.com`, `10.0.0.5`; a leading `\\` is fine) and press **Check** or Enter; 🕘 lists the
recently used computers (the last 15, remembered when a set is opened). The logs are read over the
**administrative shares** with **your current Windows account** – it needs administrator rights on
that computer (RusTrace never asks for or stores credentials; to use another account, connect
first, e.g. `net use \\pc01\admin$ /user:DOMAIN\admin`). The variables point to the remote
computer:

| Variable | This computer | Remote computer `pc01` |
|---|---|---|
| `%WINDIR%` | `C:\Windows` | `\\pc01\admin$` (its Windows folder, wherever it is installed) |
| `%CCMLOGS%` | client `LogDirectory` from the registry, else `%WINDIR%\CCM\Logs` | `\\pc01\admin$\CCM\Logs` (the default; its registry is not read) |
| `%PROGRAMDATA%` | `C:\ProgramData` | `\\pc01\c$\ProgramData` |
| `%SYSTEMDRIVE%` | `C:` | `\\pc01\c$` |
| a path `D:\Logs\x.log` | `D:\Logs\x.log` | `\\pc01\d$\Logs\x.log` |
| other `%NAME%` | environment variable | ✖ *Unknown variable* |

Every share is checked first and must answer within **10 seconds**; the window stays usable all
the time. Messages: *pc01 cannot be reached* (offline, wrong name, firewall blocks SMB), *No answer
from pc01 within 10 s*, *Access to* `\\pc01\admin$` *denied* (no administrator rights),
*The share* `\\pc01\c$` *does not exist* (administrative shares turned off); the rows show *Not reachable*,
*No answer*, *Access denied*, *Share missing*. The opened remote logs are refreshed automatically
like local ones, including rotation (`AppEnforce.log` → `AppEnforce.lo_`) – every refresh opens the
file again and reads only the new part, so the network carries only what was added. When the
computer goes offline while the logs are watched, the status bar shows the read error and the
view keeps what was read.

`%WINDIR%\WindowsUpdate.log` is not in the software updates set: since Windows 10 it only says to
run `Get-WindowsUpdateLog`.

**Variables** in the paths (case-insensitive): `%CCMLOGS%` = the `LogDirectory` of the ConfigMgr
client (`HKLM\SOFTWARE\Microsoft\CCM\Logging\@Global`), or `%WINDIR%\CCM\Logs` when it is not set;
`%WINDIR%`, `%PROGRAMDATA%`, `%SYSTEMDRIVE%`; any other environment variable works too. A path with
an unknown variable is shown as ✖ *Unknown variable*.

**Files** – like the log definitions, loaded without recompiling every time the window opens:
- **predefined**: the built-in sets; the copies in `logsets\` next to `rustrace.exe` (shipped with
  the program) replace the built-in ones with the same file name,
- **your own**: every `.set` file in `%APPDATA%\RusTrace\logsets\` (button **Own sets folder** in the
  window; Linux `~/.config/RusTrace/logsets/`), shown with "(own)"; they never replace a predefined
  set. Files that cannot be used (no `file` line, unreadable) are listed in red in the window.

**Format** – `key = value` per line, `#` starts a comment line, unknown keys and other lines are
ignored. Names and descriptions are translated **in the set file itself** (`name.<language>`,
`description.<language>` – the language code of the user interface, e.g. `sk`, `de`, `pt-BR`; then
the base language, then the plain `name` / `description`), so an own set carries its own
translations:

```
name = Application deployment
name.sk = Nasadenie aplikácií
description = Discovery, requirement evaluation and installation of applications.
description.sk = Zisťovanie, vyhodnotenie požiadaviek a inštalácia aplikácií.
file = %CCMLOGS%\AppDiscovery.log
file = %CCMLOGS%\AppEnforce.log
file = C:\MyApp\Logs\install.log
```

| Key | Meaning |
|---|---|
| `name`, `name.<language>` | name in the list (default: the file name without `.set`) |
| `description`, `description.<language>` | one or two sentences under the name |
| `file` | one log of the set (one line per log, in the order shown) |
| `guide` | optional, repeatable: a guide for the scenario, shown as a 📖 button next to the set – a file name of [docs/guides](guides/README.md) (e.g. `guide = failed-app-install.md`, opened on GitHub with its translated title), or a URL / path of your own guide (`guide = https://intranet/wiki/AppDeploy`, `guide = \\server\docs\deploy.pdf`) |

## Saving the opened logs as a set

Open the logs you need together (**File → Open**, **➕ Add log to the view**, or a set with some
logs unticked) and choose **File → Save as log set…** – it is enabled while several logs are
shown as one merged view. Enter a **name** and, if you like, a **description** that says what the
set is for; the window shows the `file` lines that will be written. The set is saved as a
`.set` file in your own sets folder and is offered in **File → Open log set…** right away; the
window title shows its name.

The paths are saved so that the set also works on **another computer**:

| Opened log | `file` line |
|---|---|
| in the ConfigMgr client log folder | `%CCMLOGS%\AppEnforce.log` |
| in the Windows folder | `%WINDIR%\Logs\CBS\CBS.log` |
| in ProgramData | `%PROGRAMDATA%\…` |
| `\\pc01\d$\Logs\app.log` (administrative share) | `D:\Logs\app.log` – on the computer chosen when opening |
| anything else | the full path as it is |

When the view mixes logs of this computer and of a remote one, or of several remote computers,
the paths of the remote ones stay as they are. Older rotated parts are not listed – they are
found again when the set is opened.

When one of your own sets already has the name (or the file name made from it is taken), the
window says so and offers **Overwrite**; enter another name to save a new set instead.
Overwriting a set of the same name keeps its `guide` lines, comments and translations. If the
view was opened from a set, its name and description are filled in, so saving again updates your
own set. A built-in set is never changed – saving under its name creates your own set beside it.

## From the command line

`rustrace --set "SCCM client"` (or `--set C:\Sets\my.set`, optionally `--computer PC01`) opens a set
without the window – see [Command line](command-line.md).
