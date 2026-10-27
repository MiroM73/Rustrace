"""Sample logs for the guide docs/guides/failed-app-install.md (and its screenshots).

Synthetic ConfigMgr client logs of one morning: the application "Contoso PDF Reader 5.2" fails to
install twice with MSI exit code 1603 (the second attempt is the scheduled retry), other
applications install fine. All names, IDs and GUIDs are invented.

Run through scripts/gen_guide_logs.py. OUT_DIR (the first argument) gets
Windows/CCM/Logs/*.log (AppDiscovery, AppEnforce, AppIntentEval, CAS, ContentTransferManager,
DataTransferService) and Windows/Logs/ContosoPDFReader_install.log (verbose MSI log). The output
is the same on every run (fixed random seed); the files' modification times are set to the time
of their last entry.
"""
import os, random, sys, uuid
from datetime import datetime, timedelta

OUT = sys.argv[1]
R = random.Random(1603)
BIAS = "-60"  # CET (UTC+1) as written by the ConfigMgr client

logs = {n: [] for n in ["AppDiscovery", "AppEnforce", "AppIntentEval", "CAS",
                        "ContentTransferManager", "DataTransferService"]}

def guid():
    return "{" + str(uuid.UUID(int=R.getrandbits(128))).upper() + "}"

SCOPE = "ScopeId_6B2E9C41-7D3A-4F18-A5C2-90E14B7D3A6F"
def app_ids(n):
    return (f"{SCOPE}/Application_{uuid.UUID(int=R.getrandbits(128))}",
            f"{SCOPE}/DeploymentType_{uuid.UUID(int=R.getrandbits(128))}")

APPS = [
    # name, technology, revision, content id, product code, ok
    ["Contoso PDF Reader 5.2", "Windows Installer (*.msi file)", 3, "Content_1f4d8c2a-5e3b-4a7f-9c61-2d8e7b0a4f13", "{8C2F1E4A-3B7D-4E59-A1C6-5D0F92B47E38}", False, "ContosoPDFReader.msi"],
    ["Fabrikam Notes 2.8", "Windows Installer (*.msi file)", 5, "Content_7a9e2b41-0c6d-4f83-b25a-e14c9d3f6a08", "{3E7A9C15-6F2B-4D81-9B4E-0A5C7D2F1B69}", True, "FabrikamNotes.msi"],
    ["Northwind Remote Agent 7.1", "Script Installer", 2, "Content_c3b5e7d9-8a1f-4e62-9d04-7f2a6b8c1e35", None, True, "Install.cmd"],
    ["Tailspin Chat 4.0", "Windows Installer (*.msi file)", 1, "Content_5d0a3f6c-2b9e-4c17-8e45-b6a1c9f2d708", "{A1D5E8B2-9C4F-4A37-8E6B-3F2C7D0A5E91}", True, "TailspinChat.msi"],
]
for a in APPS:
    a.extend(app_ids(0))  # 7 = app id, 8 = dt id

THREADS = {"AppDiscovery": [4412, 6080], "AppEnforce": [4412, 7316], "AppIntentEval": [4412, 5528],
           "CAS": [3920, 6244], "ContentTransferManager": [3920, 2716], "DataTransferService": [5104, 2716]}
FILES = {"AppDiscovery": "appdiscovery.cpp", "AppEnforce": "appexcnlib.cpp", "AppIntentEval": "appintenteval.cpp",
         "CAS": "contentaccessservice.cpp", "ContentTransferManager": "contenttransfermanager.cpp",
         "DataTransferService": "dtsjob.cpp"}

def w(log, t, msg, typ=1, comp=None, thread=None):
    comp = comp or log
    th = thread or R.choice(THREADS[log])
    tm = t.strftime("%H:%M:%S.") + f"{t.microsecond // 1000:03d}"
    line = (f'<![LOG[{msg}]LOG]!><time="{tm}{BIAS}" date="{t.month:02d}-{t.day:02d}-{t.year}" '
            f'component="{comp}" context="" type="{typ}" thread="{th}" file="{FILES[log]}:{R.randint(100, 3999)}">')
    logs[log].append((t, line))

def ms(a, b):
    return timedelta(milliseconds=R.randint(a, b))

def evaluation(t, app, state, title_state="NotInstalled"):
    name, tech, rev, _, _, _, _, appid, dtid = app
    w("AppIntentEval", t, f"ScopeId_6B2E9C41-7D3A-4F18-A5C2-90E14B7D3A6F/{dtid.split('/')[1]}.{rev} :- Current State = {state}, "
      f"Applicability = Applicable, ResolvedState = Installed, ConfigureState = NotNeeded, Title = {name}")
    return t + ms(20, 90)

def detection(t, app, found, log="AppDiscovery"):
    name, tech, rev, _, code, _, _, appid, dtid = app
    w(log, t, f"Entering ExecQueryAsync for query \"select * from CCM_AppDeliveryType where (AppDeliveryTypeId = \"{dtid}\" AND Revision = {rev})\"")
    t += ms(5, 40)
    w(log, t, f"    Performing detection of app deployment type {name} - {tech}({dtid}, revision {rev}) for system.")
    t += ms(30, 300)
    if code:
        if found:
            w(log, t, f"+++ MSI application discovered [MSI Product Code: {code}, MSI Product version: ]")
        else:
            w(log, t, f"+++ MSI application not discovered [MSI Product Code: {code}, MSI Product version: ]")
    else:
        w(log, t, f"+++ File detection {'succeeded' if found else 'failed'} for file C:\\Program Files\\Northwind\\Remote Agent\\nwagent.exe")
    t += ms(10, 60)
    word = "Discovered" if found else "Did not detect"
    w(log, t, f"+++ {word} application [AppDT Id: {dtid}, Revision: {rev}]")
    return t + ms(20, 120)

def download(t, app):
    name, tech, rev, content, *_ = app
    job = guid(); dts = guid()
    w("CAS", t, f"Requesting content {content}.1, size(KB) {R.randint(4000, 90000)}, under context System with priority Medium")
    t += ms(50, 300)
    w("CAS", t, f"Location update from CTM for content {content}.1 and request {guid()}")
    w("ContentTransferManager", t + ms(5, 30), f"CCTMJob::UpdateLocations({job}, ...). Locations: 1 DP(s) available")
    t += ms(200, 900)
    w("ContentTransferManager", t, f"CTM job {job} (corresponding DTS job {dts}) started download from 'http://dp01.contoso.local/SMS_DP_SMSPKG$/{content}.1'")
    w("DataTransferService", t + ms(30, 120), f"DTSJob {dts} created to download from 'http://dp01.contoso.local:80/SMS_DP_SMSPKG$/{content}.1' to 'C:\\Windows\\ccmcache\\{R.choice('4b5c6d7e')}'.")
    t += timedelta(seconds=R.randint(4, 25))
    w("DataTransferService", t, f"DTSJob {dts} successfully completed download.")
    t += ms(100, 400)
    w("ContentTransferManager", t, f"CTM job {job} successfully processed download completion.")
    t += ms(50, 200)
    w("CAS", t, f"Download completed for content {content}.1 under context System")
    return t + ms(200, 800)

def enforce(t, app, retry=False):
    name, tech, rev, content, code, ok, setup, appid, dtid = app
    cache = "C:\\Windows\\ccmcache\\" + {"Contoso": "4b", "Fabrikam": "5c", "Northwind": "6d", "Tailspin": "7e"}[name.split()[0]]
    th = 7316
    w("AppEnforce", t, f"+++ Starting Install enforcement for App DT \"{name} - {tech}\" ApplicationDeliveryType - {dtid}, Revision - {rev}, ContentPath - {cache}, Execution Context - System", thread=th)
    t += ms(20, 80)
    t = detection(t, app, False, log="AppEnforce")
    w("AppEnforce", t, f"+++ Application not discovered. [AppDT Id: {dtid}, Revision: {rev}]", thread=th)
    t += ms(30, 90)
    w("AppEnforce", t, f"    App enforcement environment: \n\tContext: Machine\n\tCommand line: msiexec /i \"{setup}\" /qn /l*v C:\\Windows\\Logs\\{setup[:-4]}_install.log\n\tAllow user interaction: No\n\tUI mode: 0\n\tUser token: null\n\tSession Id: 2\n\tContent path: {cache}\n\tWorking directory: " if code else
      f"    App enforcement environment: \n\tContext: Machine\n\tCommand line: \"{setup}\"\n\tAllow user interaction: No\n\tUI mode: 0\n\tUser token: null\n\tSession Id: 2\n\tContent path: {cache}\n\tWorking directory: ", thread=th)
    t += ms(5, 30)
    w("AppEnforce", t, f"    Prepared working directory: {cache}", thread=th)
    t += ms(5, 30)
    if code:
        w("AppEnforce", t, "    Found executable file msiexec with complete path C:\\Windows\\system32\\msiexec.exe", thread=th)
        cmd = f"\"C:\\Windows\\system32\\msiexec.exe\" /i \"{setup}\" /qn /l*v C:\\Windows\\Logs\\{setup[:-4]}_install.log"
    else:
        cmd = f"\"C:\\Windows\\system32\\cmd.exe\" /c \"{setup}\""
    t += ms(5, 30)
    w("AppEnforce", t, f"    Prepared command line: {cmd}", thread=th)
    t += ms(5, 30)
    w("AppEnforce", t, "    Valid MSI Package path = C:\\Windows\\ccmcache\\4b\\ContosoPDFReader.msi" if setup == "ContosoPDFReader.msi" else "    Post install behavior is BasedOnExitCode", thread=th)
    t += ms(20, 100)
    w("AppEnforce", t, f"    Executing Command line: {cmd} with system context", thread=th)
    t += ms(10, 60)
    w("AppEnforce", t, f"    Working directory {cache}", thread=th)
    t += ms(10, 60)
    w("AppEnforce", t, "    Post install behavior is BasedOnExitCode", thread=th)
    pid = R.randint(3000, 9999)
    t += ms(100, 400)
    w("AppEnforce", t, f"    Waiting for process {pid} to finish.  Timeout = 120 minutes.", thread=th)
    if ok:
        t += timedelta(seconds=R.randint(25, 70), milliseconds=R.randint(0, 999))
        w("AppEnforce", t, f"    Process {pid} terminated with exitcode: 0", thread=th)
        t += ms(10, 40)
        w("AppEnforce", t, "    Looking for exit code 0 in exit codes table...", thread=th)
        t += ms(5, 20)
        w("AppEnforce", t, "    Matched exit code 0 to a Success entry in exit codes table.", thread=th)
        t += ms(10, 40)
        t = detection(t, app, True, log="AppEnforce")
        w("AppEnforce", t, f"++++++ App enforcement completed ({R.randint(30, 80)} seconds) for App DT \"{name} - {tech}\" [{dtid}], Revision: {rev}, User SID: ] ++++++", thread=th)
    else:
        start = t
        t += timedelta(minutes=3, seconds=37, milliseconds=R.randint(0, 999))
        w("AppEnforce", t, f"    Process {pid} terminated with exitcode: 1603", thread=th)
        t += ms(10, 40)
        w("AppEnforce", t, "    Looking for exit code 1603 in exit codes table...", thread=th)
        t += ms(5, 20)
        w("AppEnforce", t, "    Unmatched exit code (1603) is considered an execution failure.", 3, thread=th)
        t += ms(10, 40)
        w("AppEnforce", t, f"++++++ App enforcement completed ({int((t - start).total_seconds()) + 1} seconds) for App DT \"{name} - {tech}\" [{dtid}], Revision: {rev}, User SID: ] ++++++", thread=th)
    return t + ms(100, 500)

def background(t0, t1):
    """Periodic re-evaluation noise: global evaluation cycles of the installed apps."""
    t = t0
    while t < t1:
        for app in APPS[1:]:
            if t > t1:
                break
            t = detection(t, app, True)
            t = evaluation(t, app, "Installed", "Installed")
        w("AppIntentEval", t, "Global evaluation cycle completed: 0 deployments require action.")
        t += timedelta(minutes=R.randint(9, 16), seconds=R.randint(0, 59))

day = datetime(2026, 10, 27)
background(day.replace(hour=5, minute=2), day.replace(hour=7, minute=10))

def deployment(t, app, first=True):
    name = app[0]
    w("AppIntentEval", t, f"Evaluating deployment of {name} (required, deadline reached).")
    t += ms(50, 200)
    t = detection(t, app, False)
    t = evaluation(t, app, "NotInstalled")
    if first:
        t = download(t, app)
    t = enforce(t, app)
    if app[5]:
        t = detection(t, app, True)
        t = evaluation(t, app, "Installed")
    else:
        t = detection(t, app, False)
        t = evaluation(t, app, "Error")
        w("AppIntentEval", t, f"Deployment of {name} failed; the next retry is scheduled in 120 minutes.", 2)
        t += ms(50, 200)
    return t

t = day.replace(hour=7, minute=14, second=3, microsecond=118000)
t = deployment(t, APPS[1])
t = deployment(t + timedelta(seconds=40), APPS[3])
t = deployment(t + timedelta(seconds=25), APPS[0])
first_fail_end = t
background(t + timedelta(minutes=6), day.replace(hour=9, minute=12))
t = day.replace(hour=9, minute=14, second=7, microsecond=402000)
t = deployment(t, APPS[0], first=False)
t = deployment(t + timedelta(seconds=55), APPS[2])
background(t + timedelta(minutes=4), day.replace(hour=10, minute=30))

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

# Verbose MSI log of the second attempt (excerpt-like, invented content).
msi = os.path.join(OUT, "Windows", "Logs")
os.makedirs(msi, exist_ok=True)
L = []
def m(ts, txt):
    L.append(f"MSI (s) (5C:A8) [{ts}]: {txt}")
L.append("=== Verbose logging started: 27.10.2026  09:14:09  Build type: SHIP UNICODE 5.00.10011.00  Calling process: C:\\Windows\\system32\\msiexec.exe ===")
m("09:14:09:512", "Resetting cached policy values")
m("09:14:09:513", "Machine policy value 'Debug' is 0")
m("09:14:09:530", "Product: Contoso PDF Reader 5.2 -- Installation started.")
L.append("Action start 9:14:10: INSTALL.")
L.append("Action start 9:14:10: LaunchConditions.")
L.append("Action ended 9:14:10: LaunchConditions. Return value 1.")
L.append("Action start 9:14:11: CostFinalize.")
L.append("Action ended 9:14:11: CostFinalize. Return value 1.")
L.append("Action start 9:14:12: InstallFiles.")
L.append("Action ended 9:17:38: InstallFiles. Return value 1.")
L.append("Action start 9:17:39: RegisterContosoShellExtension.")
m("09:17:41:204", "Executing op: CustomActionSchedule(Action=RegisterContosoShellExtension,ActionType=3073,Source=BinaryData,Target=RegisterShellExt,)")
L.append("CustomAction RegisterContosoShellExtension returned actual error code 1722 (note this may not be 100% accurate if translation happened inside sandbox)")
L.append("Error 1722. There is a problem with this Windows Installer package. A program run as part of the setup did not finish as expected. Contact your support personnel or package vendor.  Action RegisterContosoShellExtension, location: C:\\Program Files\\Contoso\\PDF Reader\\, command: regsvr32.exe /s ContosoShellExt.dll")
L.append("Action ended 9:17:44: RegisterContosoShellExtension. Return value 3.")
L.append("Action ended 9:17:44: INSTALL. Return value 3.")
m("09:17:44:871", "Product: Contoso PDF Reader 5.2 -- Installation failed.")
m("09:17:44:873", "Windows Installer installed the product. Product Name: Contoso PDF Reader 5.2. Product Version: 5.2.0.118. Product Language: 1033. Manufacturer: Contoso Ltd.. Installation success or error status: 1603.")
L.append("=== Verbose logging stopped: 27.10.2026  09:17:44 ===")
p = os.path.join(msi, "ContosoPDFReader_install.log")
with open(p, "w", encoding="utf-8", newline="\r\n") as f:
    f.write("\n".join(L) + "\n")
mt = datetime(2026, 10, 27, 9, 17, 44).timestamp()
os.utime(p, (mt, mt))
print(f"{p}: {len(L)} lines")
