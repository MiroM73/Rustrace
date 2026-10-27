"""Sample logs for the guide docs/guides/software-updates.md (and its screenshots).

Synthetic ConfigMgr client logs + CBS.log of one morning: the 07:30 software update scan fails
because the WSUS server answers HTTP 503 (0x80244022), the retry at 08:00 succeeds. At the
deadline (09:00) of the deployment "Microsoft Software Updates - 2026-10" two updates are
installed: the .NET update succeeds (restart required), the cumulative update fails because
there is not enough free disk space (0x80070070, CBS: required / available MB). All names, KB
numbers, IDs and GUIDs are invented.

Run through scripts/gen_guide_logs.py. OUT_DIR (the first argument) gets
Windows/CCM/Logs/*.log (WUAHandler, ScanAgent, UpdatesDeployment, UpdatesHandler, UpdatesStore)
and Windows/Logs/CBS/CBS.log. The output is the same on every run (fixed random seed); the
files' modification times are set to the time of their last entry.
"""
import os, random, sys, uuid
from datetime import datetime, timedelta

OUT = sys.argv[1]
R = random.Random(80070070)
BIAS = "-60"  # CET (UTC+1) as written by the ConfigMgr client

CM = ["WUAHandler", "ScanAgent", "UpdatesDeployment", "UpdatesHandler", "UpdatesStore"]
logs = {n: [] for n in CM}
cbs = []

THREADS = {"WUAHandler": [5216, 7740], "ScanAgent": [3908, 6512], "UpdatesDeployment": [4480, 6612],
           "UpdatesHandler": [4480, 7096], "UpdatesStore": [3908, 6512]}
FILES = {"WUAHandler": "cwuahandler.cpp", "ScanAgent": "cscanjob.cpp", "UpdatesDeployment": "updatesmanager.cpp",
         "UpdatesHandler": "cdeploymentjob.cpp", "UpdatesStore": "updatesstore.cpp"}

def guid():
    return "{" + str(uuid.UUID(int=R.getrandbits(128))).upper() + "}"

def lguid():
    return str(uuid.UUID(int=R.getrandbits(128)))

def w(log, t, msg, typ=1, thread=None):
    th = thread or R.choice(THREADS[log])
    tm = t.strftime("%H:%M:%S.") + f"{t.microsecond // 1000:03d}"
    line = (f'<![LOG[{msg}]LOG]!><time="{tm}{BIAS}" date="{t.month:02d}-{t.day:02d}-{t.year}" '
            f'component="{log}" context="" type="{typ}" thread="{th}" file="{FILES[log]}:{R.randint(100, 3999)}">')
    logs[log].append((t, line))

def c(t, level, comp, msg):
    cbs.append((t, f"{t.strftime('%Y-%m-%d %H:%M:%S')}, {level:<22}{comp:<7}{msg}"))

def ms(a, b):
    return timedelta(milliseconds=R.randint(a, b))

WSUS = "http://wsus01.contoso.local:8530"
SITE = "Site_7C4A1E2B-93D5-4F60-B8A7-2E1D6C9F0A35"
CU = {"title": "2026-10 Cumulative Update for Windows 11 Version 24H2 for x64-based Systems (KB5099991)",
      "kb": "5099991", "id": f"{SITE}/SUM_{lguid()}", "pkg": "Package_for_RollupFix~31bf3856ad364e35~amd64~~26100.6911.1.7",
      "size": 1311}
NET = {"title": "2026-10 Cumulative Update for .NET Framework 3.5 and 4.8.1 for Windows 11 Version 24H2 for x64 (KB5099874)",
       "kb": "5099874", "id": f"{SITE}/SUM_{lguid()}", "size": 84}
UPD = [NET, CU]
DEPLOY = guid()

# ---------------------------------------------------------------- background
def csi_noise(t, n):
    for _ in range(n):
        c(t, "Info", "CSI", f"{R.randint(0, 0xffff):08x} " + R.choice([
            "Loaded Servicing Stack v10.0.26100.6899 with Core: C:\\Windows\\winsxs\\amd64_microsoft-windows-servicingstack_31bf3856ad364e35_10.0.26100.6899_none_5a1c\\cbscore.dll",
            "@" + t.strftime("%Y/%m/%d:%H:%M:%S") + ".%03d WcpInitialize: wcp.dll version 0.0.0.6 (bfe3a2b9)" % R.randint(0, 999),
            "Perf: LookupCookie called, cookie list length is 0",
            "StoreCorruptionRepair transaction begun. WcpVersion: [l:36]'10.0.26100.6899 (WinBuild.160101.0800)'.",
            "Direct SIL provider: Number of files opened: 12.",
            "CSI Transaction @0x1d2b4c8e3f0 initialized for deployment engine {d16d444c-56d8-11d5-882d-0080c847b195} with flags 00000002 and client id [10]'TI6.0_0_0'",
        ]))
        t += ms(3, 60)
    return t

def cbs_idle(t):
    c(t, "Info", "CBS", "TI: --- Initializing Trusted Installer ---")
    t += ms(5, 40)
    c(t, "Info", "CBS", "Starting TrustedInstaller initialization.")
    t += ms(10, 80)
    c(t, "Info", "CBS", "Ending TrustedInstaller initialization.")
    t = csi_noise(t + ms(10, 80), R.randint(2, 5))
    c(t, "Info", "CBS", "Session: 31054712_" + str(R.randint(1000000000, 3999999999)) + " initialized by client WindowsUpdateAgent, external staging directory: (null), external registry directory: (null)")
    t = csi_noise(t + ms(10, 80), R.randint(2, 6))
    c(t, "Info", "CBS", "Appl: detect Parent, Package: Package_for_RollupFix~31bf3856ad364e35~amd64~~26100.6584.1.9, Parent: Microsoft-Windows-Client-Desktop-Required-Package~31bf3856ad364e35~amd64~~10.0.26100.6584, Disposition = Detect, VersionComp: EQ, BuildComp: EQ, RevisionComp: GE, Exist: present")
    t = csi_noise(t + ms(10, 80), R.randint(1, 4))
    c(t, "Info", "CBS", "Trusted Installer is shutting down because: SHUTDOWN_REASON_AUTOSTOP")
    return t

# ---------------------------------------------------------------- scan
def scan(t, ok):
    job = guid()
    w("ScanAgent", t, f"Message received: '<?xml version='1.0' ?><ScanRequest ScanType='Scheduled' Category='SoftwareUpdate' SourceType='WSUS'/>'")
    t += ms(20, 90)
    w("ScanAgent", t, f"ScanJob({job}): CScanJob::Initialize - ScanType = Scheduled, SourceType = WSUS, Location = {WSUS}")
    t += ms(30, 200)
    w("ScanAgent", t, f"ScanJob({job}): CScanJob::Scan - Scan COM call initiated")
    t += ms(50, 300)
    w("WUAHandler", t, f"Its a WSUS Update Source type ({{8A2C5F1D-4B6E-4D39-A7E0-1F3C9B5D2E68}}), adding it.")
    t += ms(20, 80)
    w("WUAHandler", t, f"Existing WUA Managed server was already set ({WSUS}), skipping Group Policy registration.")
    t += ms(20, 80)
    w("WUAHandler", t, "Async searching of updates using WUAgent started.")
    if not ok:
        t += timedelta(seconds=R.randint(40, 70), milliseconds=R.randint(0, 999))
        w("WUAHandler", t, "OnSearchComplete - Failed to end search job. Error = 0x80244022.", 3)
        t += ms(5, 30)
        w("WUAHandler", t, "Scan failed with error = 0x80244022.", 3)
        t += ms(50, 200)
        w("ScanAgent", t, f"ScanJob({job}): CScanJob::OnScanComplete -Scan Failed with Error=0x80244022", 3)
        t += ms(20, 80)
        w("ScanAgent", t, f"ScanJob({job}): CScanJob::ScheduleScanRetry- ScanRetry Timer task successfully scheduled. Will retry scan in 30 minutes", 2)
        return t
    t += timedelta(seconds=R.randint(25, 45), milliseconds=R.randint(0, 999))
    for u in UPD:
        w("WUAHandler", t, f"Update (Missing): {u['title']} ({lguid()}, 200)")
        t += ms(2, 15)
    t += ms(20, 80)
    w("WUAHandler", t, "Async searching completed.")
    t += ms(20, 80)
    w("WUAHandler", t, "Successfully completed scan.")
    t += ms(50, 200)
    w("ScanAgent", t, f"ScanJob({job}): CScanJob::OnScanComplete - Scan completed successfully, ScanType=1")
    t += ms(50, 300)
    for u in UPD:
        w("UpdatesStore", t, f"Processed update status for update ({u['id'][-36:]}), KB{u['kb']}: Status = Missing, Revision = 200")
        t += ms(10, 40)
    return t

def background(t0, t1):
    t = t0
    while t < t1:
        t = cbs_idle(t)
        w("UpdatesDeployment", t + ms(100, 900), "Evaluation timer triggered: no assignments with a deadline reached.")
        t += timedelta(minutes=R.randint(11, 19), seconds=R.randint(0, 59))

day = datetime(2026, 10, 27)
background(day.replace(hour=5, minute=4), day.replace(hour=7, minute=29))
t = scan(day.replace(hour=7, minute=30, second=2, microsecond=341000), False)
background(t + timedelta(minutes=3), day.replace(hour=7, minute=59))
t = scan(day.replace(hour=8, minute=0, second=4, microsecond=118000), True)
background(t + timedelta(minutes=4), day.replace(hour=8, minute=59))

# ---------------------------------------------------------------- deployment at the deadline
t = day.replace(hour=9, minute=0, second=0, microsecond=212000)
w("UpdatesDeployment", t, f"Deadline received for assignment ({DEPLOY}) Microsoft Software Updates - 2026-10")
t += ms(20, 80)
w("UpdatesDeployment", t, f"Assignment {DEPLOY} has total CI = 2")
for u in UPD:
    t += ms(5, 30)
    w("UpdatesDeployment", t, f"Update ({u['id']}) Name ({u['title']}) ArticleID ({u['kb']}) added to the targeted list of deployment ({DEPLOY})")
job = guid()
t += ms(50, 200)
w("UpdatesDeployment", t, f"EnforcementJob({job}) started for assignment ({DEPLOY})")
for u in UPD:
    t += ms(30, 150)
    w("UpdatesHandler", t, f"Starting download on action (INSTALL) for Update ({u['id']})")
    t += ms(5, 40)
    w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({u['id']}) Progress: Status = ciStateDownloading, PercentComplete = 0, DownloadSize = {u['size'] * 1024}, Result = 0x0")
for u in UPD:
    t += timedelta(seconds=R.randint(40, 70) if u is NET else R.randint(560, 640), milliseconds=R.randint(0, 999))
    w("UpdatesHandler", t, f"Download complete for update ({u['id']}), size {u['size']} MB")
    t += ms(5, 40)
    w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({u['id']}) Progress: Status = ciStateWaitInstall, PercentComplete = 100, DownloadSize = {u['size'] * 1024}, Result = 0x0")
t += ms(200, 900)
w("UpdatesHandler", t, "Starting install of updates (2) for assignment " + DEPLOY)
t += ms(50, 200)
w("WUAHandler", t, "Async installation of updates started.")
# .NET first
t += ms(50, 200)
w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({NET['id']}) Progress: Status = ciStateInstalling, PercentComplete = 0, Result = 0x0")
t += ms(200, 600)
c(t, "Info", "CBS", "Exec: Processing started.  Client: WindowsUpdateAgent, Session: 31054712_3311780061, Package: Package_for_DotNetRollup_481~31bf3856ad364e35~amd64~~10.0.9312.3")
t = csi_noise(t + ms(20, 90), 8)
t += timedelta(seconds=R.randint(80, 110))
c(t, "Info", "CBS", "Exec: Installation of Package_for_DotNetRollup_481~31bf3856ad364e35~amd64~~10.0.9312.3 succeeded, a reboot is required to complete the operation. [HRESULT = 0x00000000 - S_OK]")
t += ms(100, 400)
w("WUAHandler", t, f"Update 1 (ID: {lguid()}) finished installing (0x00000000), Reboot Required? Yes")
t += ms(20, 90)
w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({NET['id']}) Progress: Status = ciStatePendingSoftReboot, PercentComplete = 100, Result = 0x0")
# CU fails – not enough disk space
t += ms(200, 700)
w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({CU['id']}) Progress: Status = ciStateInstalling, PercentComplete = 0, Result = 0x0")
t += ms(300, 900)
c(t, "Info", "CBS", f"Exec: Processing started.  Client: UpdateAgentLCU, Session: 31054712_3311795507, Package: {CU['pkg']}")
t = csi_noise(t + ms(20, 90), 4)
c(t, "Info", "CBS", f"Exec: Calculating the free space required for staging the execution chain of {CU['pkg']}")
t += timedelta(seconds=R.randint(8, 14), milliseconds=R.randint(0, 999))
c(t, "Info", "CBS", "Exec: Free space on C: 2143 MB, required for staging and installing: 6912 MB")
t += ms(20, 90)
c(t, "Error", "CBS", "Exec: Not enough disk space to complete the operation. [HRESULT = 0x80070070 - ERROR_DISK_FULL]")
t += ms(5, 30)
c(t, "Error", "CBS", "Failed to stage execution chain. [HRESULT = 0x80070070 - ERROR_DISK_FULL]")
t += ms(5, 30)
c(t, "Info", "CBS", f"Exec: Processing complete.  Session: 31054712_3311795507, Package: {CU['pkg']}, Install state: Staged [HRESULT = 0x80070070 - ERROR_DISK_FULL]")
t += ms(5, 30)
c(t, "Error", "CBS", "Failed to perform operation.  [HRESULT = 0x80070070 - ERROR_DISK_FULL]")
t += ms(200, 600)
w("WUAHandler", t, f"Update 2 (ID: {lguid()}) finished installing (0x80070070), Reboot Required? No", 3)
t += ms(20, 90)
w("WUAHandler", t, "Async install completed.")
t += ms(20, 90)
w("WUAHandler", t, "Installation of updates completed with error 0x80070070.", 3)
t += ms(50, 200)
w("UpdatesHandler", t, f"Update ({CU['id']}) failed to install: 0x80070070", 3)
t += ms(10, 60)
w("UpdatesDeployment", t, f"EnforcementJob({job}) Update ({CU['id']}) Progress: Status = ciStateError, PercentComplete = 0, Result = 0x80070070", 3)
t += ms(10, 60)
w("UpdatesDeployment", t, f"Job error (0x80070070) received for assignment ({DEPLOY}) action", 3)
t += ms(20, 90)
w("UpdatesDeployment", t, f"EnforcementJob({job}) finished: 1 of 2 updates installed, 1 failed. A restart is required to complete the installation.", 2)
t += ms(50, 200)
w("UpdatesStore", t, f"Processed update status for update ({NET['id'][-36:]}), KB{NET['kb']}: Status = Installed (pending reboot), Revision = 200")
t += ms(10, 40)
w("UpdatesStore", t, f"Processed update status for update ({CU['id'][-36:]}), KB{CU['kb']}: Status = Missing, Revision = 200")
t = csi_noise(t + timedelta(seconds=5), 3)
c(t, "Info", "CBS", "Trusted Installer is shutting down because: SHUTDOWN_REASON_AUTOSTOP")
background(t + timedelta(minutes=6), day.replace(hour=10, minute=40))

# ---------------------------------------------------------------- write
base = os.path.join(OUT, "Windows", "CCM", "Logs")
os.makedirs(base, exist_ok=True)
for n, rows in logs.items():
    rows.sort(key=lambda r: r[0])
    p = os.path.join(base, n + ".log")
    with open(p, "w", encoding="utf-8", newline="\r\n") as f:
        for _, line in rows:
            f.write(line + "\n")
    mt = rows[-1][0].timestamp()
    os.utime(p, (mt, mt))
    print(f"{p}: {len(rows)} entries")
cbs.sort(key=lambda r: r[0])
d = os.path.join(OUT, "Windows", "Logs", "CBS")
os.makedirs(d, exist_ok=True)
p = os.path.join(d, "CBS.log")
with open(p, "w", encoding="utf-8", newline="\r\n") as f:
    for _, line in cbs:
        f.write(line + "\n")
mt = cbs[-1][0].timestamp()
os.utime(p, (mt, mt))
print(f"{p}: {len(cbs)} lines")
