"""Sample logs for the guide docs/guides/client-install.md (and its screenshots).

Synthetic ConfigMgr client installation of one morning: ccmsetup downloads and installs the client
from MP01.contoso.local (exit code 0, client.msi succeeds), but then the client gets the old
name of its management point (mp01.corp.contoso.local) from Active Directory, cannot resolve it
(0x80072EE7) and stays unregistered; registration retries back off. At 10:15 a DNS record is
added and the next retry registers the client. LocationServices.log rotates on the way, so an
older part LocationServices.lo_ exists. All names, IDs and GUIDs are invented.

Run through scripts/gen_guide_logs.py. OUT_DIR (the first argument) gets
Windows/ccmsetup/Logs/ccmsetup.log, Windows/ccmsetup/Logs/client.msi.log (verbose MSI log),
Windows/CCM/Logs/ClientIDManagerStartup.log, Windows/CCM/Logs/LocationServices.log and
Windows/CCM/Logs/LocationServices.lo_. The output is the same on every run (fixed random seed);
the files' modification times are set to the time of their last entry.
"""
import os, random, sys, uuid
from datetime import datetime, timedelta

OUT = sys.argv[1]
R = random.Random(0x80072EE7)
BIAS = "-60"  # CET (UTC+1) as written by the ConfigMgr client

logs = {n: [] for n in ["ccmsetup", "ClientIDManagerStartup", "LocationServices"]}
msi = []
THREADS = {"ccmsetup": [4120, 4388], "ClientIDManagerStartup": [3316, 5872], "LocationServices": [3316, 6020]}
FILES = {"ccmsetup": "ccmsetup.cpp", "ClientIDManagerStartup": "regtask.cpp", "LocationServices": "lsad.cpp"}
OLD = "mp01.corp.contoso.local"
NEW = "MP01.contoso.local"
GUID = "GUID:" + str(uuid.UUID(int=R.getrandbits(128))).upper()

def w(log, t, msg, typ=1, comp=None):
    comp = comp or log
    tm = t.strftime("%H:%M:%S.") + f"{t.microsecond // 1000:03d}"
    line = (f'<![LOG[{msg}]LOG]!><time="{tm}{BIAS}" date="{t.month:02d}-{t.day:02d}-{t.year}" '
            f'component="{comp}" context="" type="{typ}" thread="{R.choice(THREADS[log])}" file="{FILES[log]}:{R.randint(100, 3999)}">')
    logs[log].append((t, line))

def ms(a, b):
    return timedelta(milliseconds=R.randint(a, b))

def m(t, txt):
    msi.append(f"MSI (s) (2C:{R.choice(['A4', 'B8', '1C'])}) [{t.strftime('%H:%M:%S')}:{t.microsecond // 1000:03d}]: {txt}")

day = datetime(2026, 10, 28)

# ---------------------------------------------------------------- ccmsetup + client.msi
t = day.replace(hour=8, minute=2, second=11, microsecond=406000)
for msg in [
    "==========[ ccmsetup started in process 4120 ]==========",
    "Running on 'Microsoft Windows 11 Enterprise' (10.0.26100). Service Pack (0.0). SuiteMask = 272. Product Type = 18",
    f"Ccmsetup command line: \"C:\\WINDOWS\\ccmsetup\\ccmsetup.exe\" /mp:{NEW} SMSSITECODE=CTS",
    "Command line parameters for ccmsetup have been specified. No registry lookup for command line parameters is required.",
    "SslState value: 224",
    "CCMHTTPPORT:    80",
    "CCMHTTPSPORT:    443",
    f"Lookup MP(s) from command line: {NEW}",
    "Detected 18423 MB free disk space on system drive.",
    "Checking Write Filter Status.",
    "This is not a supported write filter device. We are not in a write filter maintenance mode.",
    f"Downloading file https://{NEW}/CCM_Client/ccmsetup.cab",
]:
    w("ccmsetup", t, msg); t += ms(20, 300)
for p in (16, 52, 100):
    t += ms(400, 1500); w("ccmsetup", t, f"File download {p}% complete ({p * 2201 // 100} of 2201 KB).")
w("ccmsetup", t + ms(10, 50), "Download complete."); t += ms(100, 400)
w("ccmsetup", t, "Successfully extracted manifest file C:\\WINDOWS\\ccmsetup\\ccmsetupmanifest.xml from file C:\\WINDOWS\\ccmsetup\\ccmsetup.cab."); t += ms(50, 200)
w("ccmsetup", t, "Retrieved client version '5.00.9135.1000' and minimum assignable site version '5.00.8540.1000' from client package"); t += ms(50, 200)
w("ccmsetup", t, f"Downloading file https://{NEW}/CCM_Client/x64/client.msi"); t += ms(100, 300)
for p in (9, 33, 61, 88, 100):
    t += ms(2000, 6000); w("ccmsetup", t, f"File download {p}% complete ({p * 74301 // 100} of 74301 KB).")
w("ccmsetup", t + ms(10, 50), "Download complete."); t += timedelta(seconds=R.randint(20, 40))
w("ccmsetup", t, "Installing file 'C:\\WINDOWS\\ccmsetup\\client.msi' with options 'SMSSITECODE=CTS CCMFIRSTCERT=1'"); t += ms(30, 90)
w("ccmsetup", t, "Running installation package\n  Package:     C:\\WINDOWS\\ccmsetup\\client.msi\n  Log:         C:\\WINDOWS\\ccmsetup\\Logs\\client.msi.log\n  Properties:  SMSSITECODE=CTS CCMFIRSTCERT=1")
mt = t + ms(300, 800)
msi.append(f"=== Verbose logging started: 28.10.2026  {mt.strftime('%H:%M:%S')}  Build type: SHIP UNICODE 5.00.10011.00  Calling process: C:\\WINDOWS\\ccmsetup\\ccmsetup.exe ===")
m(mt, "Resetting cached policy values"); mt += ms(1, 20)
m(mt, "Machine policy value 'Debug' is 0"); mt += ms(5, 50)
m(mt, "Product: Configuration Manager Client -- Installation started."); mt += ms(100, 400)
for a in ["LaunchConditions", "SmsDetectColdBoot", "CcmCheckPrereqs", "CostInitialize", "FileCost", "CostFinalize", "InstallValidate"]:
    msi.append(f"Action start {mt.hour}:{mt:%M:%S}: {a}.")
    mt += ms(100, 900)
    if a == "FileCost":
        m(mt, "Note: 1: 2205 2:  3: Error ")
        m(mt + ms(1, 5), "Note: 1: 2228 2:  3: Error 4: SELECT `Message` FROM `Error` WHERE `Error` = 1707 ")
    msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: {a}. Return value 1.")
msi.append(f"Action start {mt.hour}:{mt:%M:%S}: InstallFiles.")
mt += timedelta(minutes=2, seconds=R.randint(10, 50))
msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: InstallFiles. Return value 1.")
for a in ["CcmRegisterWmiMofs", "CcmRegisterPerfCounters", "CcmCreateSiteAssignment", "CcmStartService"]:
    msi.append(f"Action start {mt.hour}:{mt:%M:%S}: {a}.")
    mt += timedelta(seconds=R.randint(4, 40), milliseconds=R.randint(0, 999))
    msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: {a}. Return value 1.")
m(mt, "Product: Configuration Manager Client -- Installation completed successfully."); mt += ms(1, 20)
m(mt, "Windows Installer installed the product. Product Name: Configuration Manager Client. Product Version: 5.00.9135.1000. Product Language: 1033. Manufacturer: Microsoft Corporation. Installation success or error status: 0.")
mt += ms(10, 90)
msi.append(f"=== Verbose logging stopped: 28.10.2026  {mt.strftime('%H:%M:%S')} ===")
msi_end = mt
t = mt + ms(300, 900)
w("ccmsetup", t, "Installation succeeded."); t += ms(20, 90)
w("ccmsetup", t, "Client installation completed successfully."); t += ms(30, 120)
w("ccmsetup", t, "CcmSetup is exiting with return code 0")
setup_end = t

# ---------------------------------------------------------------- client start, MP lookup, registration
t = setup_end + timedelta(seconds=R.randint(8, 15))
w("ClientIDManagerStartup", t, "[----- SMS client is starting -----]"); t += ms(100, 400)
w("ClientIDManagerStartup", t, "Computed HardwareID=2:1F4E8B27C90D53A6E2B1704F8D3C65A91E07B2F4"); t += ms(30, 200)
w("ClientIDManagerStartup", t, "Machine: PC0815, Hardware ID: 2:1F4E8B27C90D53A6E2B1704F8D3C65A91E07B2F4"); t += ms(30, 200)
w("ClientIDManagerStartup", t, f"[RegTask] - Client is not registered. Sending registration request for {GUID} ..."); t += ms(100, 300)
w("LocationServices", t, "Current AD site of machine is BRNO-Branch"); t += ms(30, 100)
w("LocationServices", t, f"Assigned MP changed from <> to <{NEW}>."); t += ms(30, 100)
w("ClientIDManagerStartup", t + ms(400, 900), "[RegTask] - Client registration is pending. Server assigned ClientID is " + GUID)
t += timedelta(seconds=R.randint(3, 8))
w("LocationServices", t, "Attempting to retrieve default management points from AD"); t += ms(100, 600)
w("LocationServices", t, f"Retrieved MP [{OLD}] from AD for site CTS"); t += ms(20, 80)
w("LocationServices", t, f"Assigned MP changed from <{NEW}> to <{OLD}>.", 2); t += ms(20, 80)
w("LocationServices", t, f"Persisted Default Management Point Locations locally"); t += ms(100, 400)

fix = day.replace(hour=10, minute=15, second=0)
delay, attempt = 60, 1
t += timedelta(seconds=R.randint(30, 50))
while True:
    w("ClientIDManagerStartup", t, f"[RegTask] - Executing registration task synchronously."); t += ms(20, 90)
    w("ClientIDManagerStartup", t, f"[RegTask] - Client registration is pending. Sending confirmation request for {GUID} ..."); t += ms(50, 200)
    if t < fix:
        w("LocationServices", t, f"Failed to resolve the name of the management point {OLD}. Error 0x80072ee7", 3); t += ms(5, 30)
        w("ClientIDManagerStartup", t, "[RegTask] - Failed to send registration request message. Error: 0x80072ee7", 3); t += ms(5, 30)
        w("ClientIDManagerStartup", t, f"[RegTask] - Sleeping for {delay} seconds ...", 2)
        t += timedelta(seconds=delay, milliseconds=R.randint(0, 999))
        delay, attempt = min(delay * 2, 1920), attempt + 1
        # periodic MP list refresh in between
        if attempt % 2 == 0:
            w("LocationServices", t - timedelta(seconds=R.randint(5, 20)), f"Refreshing the Management Point List for Site CTS Location Service")
            w("LocationServices", t - timedelta(seconds=R.randint(1, 4)), f"Failed to send management point list Location Request Message to {OLD}", 2)
            w("LocationServices", t - timedelta(seconds=R.randint(0, 1)), "Current AD site of machine is BRNO-Branch")
    else:
        w("LocationServices", t, f"Resolved {OLD} to 10.10.1.21"); t += ms(100, 600)
        w("ClientIDManagerStartup", t, f"[RegTask] - Client is registered. Server assigned ClientID is {GUID}. Approval status 1"); t += ms(50, 200)
        w("LocationServices", t, f"Refreshing the Management Point List for Site CTS Location Service"); t += ms(300, 900)
        w("LocationServices", t, f"Retrieved management point list: {OLD} (HTTPS), {NEW} (HTTPS)"); t += ms(20, 80)
        w("ClientIDManagerStartup", t, "[----- Client registration completed -----]")
        break
end = t
# some normal location refreshes later
for k in range(3):
    t += timedelta(minutes=R.randint(14, 22), seconds=R.randint(0, 59))
    w("LocationServices", t, "Refreshing the Management Point List for Site CTS Location Service")
    w("LocationServices", t + ms(200, 900), f"Retrieved management point list: {OLD} (HTTPS), {NEW} (HTTPS)")

# ---------------------------------------------------------------- write (LocationServices rotated at 09:20)
def write(path, rows):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    rows.sort(key=lambda r: r[0])
    with open(path, "w", encoding="utf-8", newline="\r\n") as f:
        for _, line in rows:
            f.write(line + "\n")
    mt = rows[-1][0].timestamp()
    os.utime(path, (mt, mt))
    print(f"{path}: {len(rows)} entries")

ccm = os.path.join(OUT, "Windows", "CCM", "Logs")
setup = os.path.join(OUT, "Windows", "ccmsetup", "Logs")
write(os.path.join(setup, "ccmsetup.log"), logs["ccmsetup"])
write(os.path.join(ccm, "ClientIDManagerStartup.log"), logs["ClientIDManagerStartup"])
rot = day.replace(hour=9, minute=20)
ls = sorted(logs["LocationServices"], key=lambda r: r[0])
write(os.path.join(ccm, "LocationServices.lo_"), [r for r in ls if r[0] < rot])
write(os.path.join(ccm, "LocationServices.log"), [r for r in ls if r[0] >= rot])
p = os.path.join(setup, "client.msi.log")
with open(p, "w", encoding="utf-8", newline="\r\n") as f:
    f.write("\n".join(msi) + "\n")
os.utime(p, (msi_end.timestamp(), msi_end.timestamp()))
print(f"{p}: {len(msi)} lines")
