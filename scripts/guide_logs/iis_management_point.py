"""Sample IIS log for the guide docs/guides/iis-management-point.md (and its screenshots).

A synthetic W3C log of the site that hosts the management point and distribution point of the
site server CM01 (10.10.1.21) for one morning (06:00 – 12:00 UTC = 07:00 – 13:00 CET): about 300
clients ask for policies (CCM_POST /ccm_system/request, …/ccm_system_windowsauth/request with the
normal 401.2 → 200 Windows authentication handshake), MP lists and certificates, and download
content with BITS. Problems in it:

- 08:40 – 08:52 UTC the management point cannot reach its database: CCM_POST requests answer
  500.0 after ~30 s, then the queue fills up and requests answer 503.2;
- one client (PC0042, 10.20.7.42) fails Windows authentication all morning: 401.1 with Win32
  status 1326 (broken computer account);
- requests for a Windows image (.esd) answer 404.3 – no MIME type for the extension;
- a few downloads end with Win32 status 64 (the client closed the connection).

All names, IPs, IDs and GUIDs are invented. Run through scripts/gen_guide_logs.py. OUT_DIR (the
first argument) gets inetpub/logs/LogFiles/W3SVC1/u_ex261028.log. The output is the same on
every run (fixed random seed); the file's modification time is set to the time of its last entry.
"""
import os, random, sys, uuid
from datetime import datetime, timedelta, timezone

OUT = sys.argv[1]
R = random.Random(500_0503)
SIP = "10.10.1.21"
CLIENTS = [(f"10.20.{R.randint(1, 9)}.{R.randint(10, 250)}", f"PC{R.randint(1000, 4999)}") for _ in range(300)]
CLIENTS.append(("10.20.4.17", "PC0815"))
BAD = ("10.20.7.42", "PC0042")
OUTAGE = (datetime(2026, 10, 28, 8, 40), datetime(2026, 10, 28, 8, 52, 30))
CONTENT = [f"Content_{uuid.UUID(int=R.getrandbits(128))}.1" for _ in range(6)]
ESD = f"Content_{uuid.UUID(int=R.getrandbits(128))}.1"

rows = []

def line(t, method, stem, query, port, user, cip, agent, status, sub, win32, taken):
    rows.append((t, f"{t:%Y-%m-%d %H:%M:%S} {SIP} {method} {stem} {query or '-'} {port} {user or '-'} {cip} "
                    f"{agent} - {status} {sub} {win32} {taken}"))

def in_outage(t):
    return OUTAGE[0] <= t < OUTAGE[1]

def policy(t, cip, pc):
    """One policy request: windows auth handshake + request, anonymous request."""
    line(t, "CCM_POST", "/ccm_system_windowsauth/request", None, 443, None, cip, "ccmhttp", 401, 2, 5, R.randint(0, 3))
    t2 = t + timedelta(milliseconds=R.randint(5, 40))
    if (cip, pc) == BAD:
        line(t2, "CCM_POST", "/ccm_system_windowsauth/request", None, 443, None, cip, "ccmhttp", 401, 1, 1326, R.randint(3, 12))
        return
    user = f"CONTOSO\\{pc}$"
    if in_outage(t2):
        k = (t2 - OUTAGE[0]).total_seconds() / (OUTAGE[1] - OUTAGE[0]).total_seconds()
        if k > 0.35 and R.random() < 0.55:
            line(t2, "CCM_POST", "/ccm_system_windowsauth/request", None, 443, user, cip, "ccmhttp", 503, 2, 0, R.randint(0, 5))
        else:
            line(t2, "CCM_POST", "/ccm_system_windowsauth/request", None, 443, user, cip, "ccmhttp", 500, 0, 0, R.randint(29800, 30400))
    else:
        line(t2, "CCM_POST", "/ccm_system_windowsauth/request", None, 443, user, cip, "ccmhttp", 200, 0, 0, R.randint(15, 140))
    t3 = t2 + timedelta(seconds=R.randint(1, 4))
    if in_outage(t3):
        line(t3, "CCM_POST", "/ccm_system/request", None, 443, None, cip, "ccmhttp", 500, 0, 0, R.randint(29800, 30400))
    else:
        line(t3, "CCM_POST", "/ccm_system/request", None, 443, None, cip, "ccmhttp", 200, 0, 0, R.randint(8, 90))

def mplist(t, cip):
    for q in ("MPLIST", "MPKEYINFORMATIONEX"):
        line(t, "GET", "/SMS_MP/.sms_aut", q, 443, None, cip, "SMS+CCM+5.0", 200, 0, 0, R.randint(1, 15))
        t += timedelta(milliseconds=R.randint(30, 300))

def download(t, cip, content, name, size_ok=True):
    stem = f"/SMS_DP_SMSPKG$/{content}/{name}"
    line(t, "HEAD", stem, None, 443, None, cip, "Microsoft+BITS/7.8", 200, 0, 0, R.randint(1, 6))
    for _ in range(R.randint(1, 4)):
        t += timedelta(seconds=R.randint(1, 20))
        win32 = 64 if R.random() < 0.04 else 0
        line(t, "GET", stem, None, 443, None, cip, "Microsoft+BITS/7.8", 206, 0, win32, R.randint(200, 9000))

start = datetime(2026, 10, 28, 6, 0, 0)
end = datetime(2026, 10, 28, 12, 0, 0)
for cip, pc in CLIENTS + [BAD]:
    t = start + timedelta(seconds=R.randint(0, 900))
    while t < end:
        policy(t, cip, pc)
        if R.random() < 0.15:
            mplist(t + timedelta(seconds=R.randint(5, 30)), cip)
        if R.random() < 0.03:
            download(t + timedelta(seconds=R.randint(30, 200)), cip, R.choice(CONTENT), R.choice(["setup.msi", "data1.cab", "install.cmd", "update.exe"]))
        t += timedelta(minutes=R.randint(13, 17), seconds=R.randint(0, 59))
# Windows image download attempts (task sequence content) – 404.3
for k in range(9):
    t = datetime(2026, 10, 28, 7, 5) + timedelta(minutes=R.randint(0, 260), seconds=R.randint(0, 59))
    cip, _ = R.choice(CLIENTS)
    line(t, "HEAD", f"/SMS_DP_SMSPKG$/{ESD}/sources/install.esd", None, 443, None, cip, "Microsoft+BITS/7.8", 404, 3, 50, R.randint(0, 3))

rows.sort(key=lambda r: r[0])
path = os.path.join(OUT, "inetpub", "logs", "LogFiles", "W3SVC1", "u_ex261028.log")
os.makedirs(os.path.dirname(path), exist_ok=True)
with open(path, "w", encoding="utf-8", newline="\r\n") as f:
    f.write("#Software: Microsoft Internet Information Services 10.0\n#Version: 1.0\n#Date: 2026-10-28 06:00:00\n")
    f.write("#Fields: date time s-ip cs-method cs-uri-stem cs-uri-query s-port cs-username c-ip cs(User-Agent) cs(Referer) sc-status sc-substatus sc-win32-status time-taken\n")
    for _, l in rows:
        f.write(l + "\n")
mt = rows[-1][0].replace(tzinfo=timezone.utc).timestamp()
os.utime(path, (mt, mt))
print(f"{path}: {len(rows)} requests")
