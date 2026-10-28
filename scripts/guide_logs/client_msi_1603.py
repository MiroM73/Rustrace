"""Sample logs for the guide docs/guides/client-msi-1603.md (and its screenshots).

A ConfigMgr client installation on PC0815 that fails in client.msi:
- 08:10 the first attempt gets MSI 1618 (another installation – a Windows Update – is running),
  ccmsetup waits and retries;
- 08:21 the second attempt runs client.msi, which fails with 1603 in the custom action
  CcmRegisterHostingConfiguration (Error 25150, WMI error 80041002 – the WMI repository of the
  computer is damaged); the installation is rolled back and ccmsetup schedules the next retry.
The verbose client.msi.log contains the usual harmless "Note: 1: 2205 … Error" lines, a property
dump and the rollback, like a real one. All names, IDs and GUIDs are invented.

Run through scripts/gen_guide_logs.py. OUT_DIR (the first argument) gets
Windows/ccmsetup/Logs/ccmsetup.log and Windows/ccmsetup/Logs/client.msi.log. The output is the
same on every run (fixed random seed); the files' modification times are set to their last entry.
"""
import os, random, sys
from datetime import datetime, timedelta

OUT = sys.argv[1]
R = random.Random(1603_25150)
BIAS = "-60"
cm = []
MP = "MP01.contoso.local"
PROD = "{4F1A7C9E-2B6D-4E3A-9C80-7D5E1F2A3B46}"

def w(t, msg, typ=1):
    tm = t.strftime("%H:%M:%S.") + f"{t.microsecond // 1000:03d}"
    cm.append((t, f'<![LOG[{msg}]LOG]!><time="{tm}{BIAS}" date="{t.month:02d}-{t.day:02d}-{t.year}" '
                  f'component="ccmsetup" context="" type="{typ}" thread="{R.choice([3880, 4212])}" file="ccmsetup.cpp:{R.randint(100, 9999)}">'))

def ms(a, b):
    return timedelta(milliseconds=R.randint(a, b))

day = datetime(2026, 10, 28)
t = day.replace(hour=8, minute=9, second=40, microsecond=118000)
for m in ["==========[ ccmsetup started in process 3880 ]==========",
          "Running on 'Microsoft Windows 11 Enterprise' (10.0.26100). Service Pack (0.0). SuiteMask = 272. Product Type = 18",
          f"Ccmsetup command line: \"C:\\WINDOWS\\ccmsetup\\ccmsetup.exe\" /mp:{MP} SMSSITECODE=CTS",
          "Detected 21544 MB free disk space on system drive.",
          f"Downloading file https://{MP}/CCM_Client/ccmsetup.cab", "Download complete.",
          f"Downloading file https://{MP}/CCM_Client/x64/client.msi"]:
    w(t, m); t += ms(200, 2500)
t += timedelta(seconds=14)
w(t, "Download complete."); t += ms(100, 400)
w(t, "Installing file 'C:\\WINDOWS\\ccmsetup\\client.msi' with options 'SMSSITECODE=CTS CCMFIRSTCERT=1'"); t += ms(300, 900)
w(t, "Another installation is in progress. Windows Installer returned 1618.", 2); t += ms(10, 40)
w(t, "File C:\\WINDOWS\\ccmsetup\\client.msi installation failed. Error text: ExitCode: 1618\nAction: \nErrorMessages:\n", 3); t += ms(10, 40)
w(t, "Client installation has failed with error code 1618. Retrying in 10 minute(s).", 2)
t = t + timedelta(minutes=10, milliseconds=R.randint(0, 900))
w(t, "Retrying the client installation (attempt 2 of 3).")
t += ms(200, 800)
w(t, "Installing file 'C:\\WINDOWS\\ccmsetup\\client.msi' with options 'SMSSITECODE=CTS CCMFIRSTCERT=1'"); t += ms(30, 90)
w(t, "Running installation package\n  Package:     C:\\WINDOWS\\ccmsetup\\client.msi\n  Log:         C:\\WINDOWS\\ccmsetup\\Logs\\client.msi.log\n  Properties:  SMSSITECODE=CTS CCMFIRSTCERT=1")

# ------------------------------------------------------------ client.msi.log (attempt 2)
msi = []
mt = t + ms(300, 700)
def m(txt, ctx="s", pid="2C", tid=None):
    global mt
    tid = tid or R.choice(["A4", "B8", "1C", "60"])
    msi.append(f"MSI ({ctx}) ({pid}:{tid}) [{mt:%H:%M:%S}:{mt.microsecond // 1000:03d}]: {txt}")
    mt += ms(1, 40)
def act(name, rv=1, dur=(50, 900), body=None):
    global mt
    msi.append(f"Action start {mt.hour}:{mt:%M:%S}: {name}.")
    mt += ms(*dur)
    if body:
        body()
    msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: {name}. Return value {rv}.")
    mt += ms(5, 60)
def notes():
    for _ in range(R.randint(1, 3)):
        m(R.choice(["Note: 1: 2205 2:  3: Error ", "Note: 1: 2205 2:  3: Font ", "Note: 1: 2205 2:  3: Class ",
                    "Note: 1: 2228 2:  3: Error 4: SELECT `Message` FROM `Error` WHERE `Error` = 1707 ",
                    "Note: 1: 2262 2: AdminProperties 3: -2147287038 "]))

msi.append(f"=== Verbose logging started: 28.10.2026  {mt:%H:%M:%S}  Build type: SHIP UNICODE 5.00.10011.00  Calling process: C:\\WINDOWS\\ccmsetup\\ccmsetup.exe ===")
m("Resetting cached policy values"); m("Machine policy value 'Debug' is 0"); m("******* RunEngine:")
m("           ******* Product: C:\\WINDOWS\\ccmsetup\\client.msi"); m("           ******* Action: ")
m("           ******* CommandLine: **********"); m("Client-side and UI is none or basic: Running entire install on the server.")
m("Grabbed execution mutex."); m("Cloaking enabled."); m("Attempting to enable all disabled privileges before calling Install on Server")
m("Product: Configuration Manager Client -- Installation started.")
for a in ["INSTALL", "SetCCMSetupProperties", "SmsDetectColdBoot", "CcmCheckPrereqs", "LaunchConditions", "AppSearch",
          "CcmCheckWmiRepository", "FindRelatedProducts", "ValidateProductID", "CostInitialize", "FileCost"]:
    if a == "INSTALL":
        msi.append(f"Action start {mt.hour}:{mt:%M:%S}: INSTALL."); mt += ms(5, 30); continue
    act(a, body=notes if R.random() < 0.5 else None)
act("CostFinalize", body=notes); act("InstallValidate", dur=(300, 1500))
act("InstallInitialize", dur=(200, 900))
for a in ["CcmStopService", "RemoveExistingProducts", "StopServices", "DeleteServices", "RemoveFiles", "CreateFolders"]:
    act(a, dur=(30, 400))
def files():
    global mt
    for f in ["CcmExec.exe", "CcmCore.dll", "ccmgencert.dll", "CcmFramework.dll", "PolicyAgent.dll", "smsclient.mof", "ccmhttp.dll"]:
        m(f"Executing op: FileCopy(SourceName={f},SourceCabKey={f},DestName={f},Attributes=0,FileSize={R.randint(80000, 4000000)},PerTick=65536,IsCompressed=1,VerifyMedia=1,,,,ComponentId={{{R.getrandbits(32):08X}-...}},IsCopy=1,CopyFlags=0)")
        mt += timedelta(seconds=R.randint(2, 9))
act("InstallFiles", dur=(500, 2000), body=files)
act("CcmRegisterWmiMofs", dur=(8000, 20000))
act("CcmRegisterPerfCounters", dur=(500, 3000))
def hosting():
    global mt
    m("Doing action: CcmRegisterHostingConfiguration")
    m("Note: 1: 2205 2:  3: ActionText ")
    m("Executing op: CustomActionSchedule(Action=CcmRegisterHostingConfiguration,ActionType=3073,Source=BinaryData,Target=CcmRegisterHostingConfiguration,CustomActionData=C:\\WINDOWS\\CCM\\;...)")
    mt += timedelta(seconds=R.randint(18, 26))
    msi.append(f"[{mt:%d.%m.%Y %H:%M:%S}] Registering Hosting Configuration.")
    m("Closing MSIHANDLE (2214) of type 790531 for thread 3052")
    msi.append(f"[{mt:%d.%m.%Y %H:%M:%S}] @@ERR:25150")
    m("Product: Configuration Manager Client -- Error 25150. Setup was unable to register the CCM_Service_HostingConfiguration endpoint The error code is 80041002")
    msi.append("Error 25150. Setup was unable to register the CCM_Service_HostingConfiguration endpoint")
    msi.append("The error code is 80041002")
    m("Closing MSIHANDLE (2212) of type 790536 for thread 3052")
    msi.append("CustomAction CcmRegisterHostingConfiguration returned actual error code 1603 (note this may not be 100% accurate if translation happened inside sandbox)")
act("CcmRegisterHostingConfiguration", rv=3, dur=(100, 300), body=hosting)
msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: InstallExecute. Return value 3.")
mt += ms(10, 60)
m("Executing op: Header(Signature=1397708873,Version=500,Timestamp=1500348932,LangId=1033,Platform=589824,ScriptType=2,ScriptMajorVersion=21,ScriptMinorVersion=4,ScriptAttributes=1)")
for f in ["ccmhttp.dll", "smsclient.mof", "PolicyAgent.dll", "CcmFramework.dll", "ccmgencert.dll", "CcmCore.dll", "CcmExec.exe"]:
    m(f"Executing op: FileRemove(,FileName=C:\\WINDOWS\\CCM\\{f},,)")
    mt += ms(50, 400)
m("Rollback: Registering Hosting Configuration")
m("Rollback: Registering performance counters")
m("Rollback: Copying new files")
m("Rollback: Creating folders")
m("Note: 1: 2318 2:  ")
m("No System Restore sequence number for this installation.")
m("Unlocking Server")
msi.append(f"Action ended {mt.hour}:{mt:%M:%S}: INSTALL. Return value 3.")
mt += ms(10, 60)
m("Product: Configuration Manager Client -- Installation failed.")
m("Windows Installer installed the product. Product Name: Configuration Manager Client. Product Version: 5.00.9135.1000. Product Language: 1033. Manufacturer: Microsoft Corporation. Installation success or error status: 1603.")
for p, v in [("ProductCode", PROD), ("ProductName", "Configuration Manager Client"), ("ProductVersion", "5.00.9135.1000"),
             ("SMSSITECODE", "CTS"), ("CCMFIRSTCERT", "1"), ("INSTALLDIR", "C:\\WINDOWS\\CCM\\"), ("SourceDir", "C:\\WINDOWS\\ccmsetup\\"),
             ("ALLUSERS", "1"), ("MsiLogFileLocation", "C:\\WINDOWS\\ccmsetup\\Logs\\client.msi.log"), ("REBOOT", "ReallySuppress")]:
    msi.append(f"Property(S): {p} = {v}")
m("MainEngineThread is returning 1603")
msi.append(f"=== Verbose logging stopped: 28.10.2026  {mt:%H:%M:%S} ===")
end = mt

t = end + ms(300, 800)
w(t, "File C:\\WINDOWS\\ccmsetup\\client.msi installation failed. Error text: ExitCode: 1603\nAction: CcmRegisterHostingConfiguration.\nErrorMessages:\nSetup was unable to register the CCM_Service_HostingConfiguration endpoint\nThe error code is 80041002\n", 3); t += ms(10, 40)
w(t, "Client installation has failed with error code 1603", 3); t += ms(10, 40)
w(t, "CcmSetup failed with error code 0x80070643", 3); t += ms(10, 40)
w(t, "Next retry in 120 minute(s)...", 2); t += ms(30, 90)
w(t, "CcmSetup is exiting with return code 6")

def write(path, lines, mtime):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\r\n") as f:
        f.write("\n".join(lines) + "\n")
    os.utime(path, (mtime.timestamp(), mtime.timestamp()))
    print(f"{path}: {len(lines)} lines")

d = os.path.join(OUT, "Windows", "ccmsetup", "Logs")
write(os.path.join(d, "ccmsetup.log"), [l for _, l in cm], cm[-1][0])
write(os.path.join(d, "client.msi.log"), msi, end)
