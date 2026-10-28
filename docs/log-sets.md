# Log sets

Groups of logs for one troubleshooting scenario, opened together on this or a remote computer.

[← Back to README](../README.md)

A **log set** is a named group of logs for one troubleshooting scenario. **File → Open log set…**
opens a window: choose the set (the last one is remembered) and the computer, and the window checks
in the background which of its logs exist – ✔ *Found* with the size, the last write time
and the number of rotated older parts that are opened with it (`AppEnforce.lo_`,
`AppEnforce-20261027-101500.log`), ✖ *Not found*, 🔒 *Access denied*. Found logs are ticked; untick
the ones you do not need and press **Open** – the logs are opened as **one view merged by time**
(the same as opening several files, see [Several logs in one view](features.md#several-logs-in-one-view-sorted-by-time)). **Check again**
repeats the check (e.g. after starting the scenario on the client).

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
