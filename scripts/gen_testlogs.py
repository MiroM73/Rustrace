"""Generates test logs for RusTrace into the testlogs/ folder (not in git).

    python scripts/gen_testlogs.py              # small and big logs
    python scripts/gen_testlogs.py --small-only # only the small logs (a few seconds)

The content is random but reproducible (fixed seed). See testlogs/README.txt for the list.
"""
import argparse
import datetime as dt
import os
import random
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "testlogs"
DAY = dt.datetime(2026, 10, 23, 0, 0, 0)

COMPONENTS = ["AppEnforce", "AppDiscovery", "CcmExec", "DataTransferService", "ContentTransferManager",
              "PolicyAgent", "UpdatesHandler", "WUAHandler", "ScanAgent", "CIAgent", "LocationServices",
              "StatusAgent", "InventoryAgent", "execmgr", "CAS", "SoftwareCenter"]
WORDS = ("policy update content download job state request evaluation application deployment "
         "assignment client server software package program install success retry timeout location "
         "certificate inventory scan compliance user machine service registry value detected handler "
         "notification transfer").split()
ERRORS = ["0x80070005", "0x80004005", "0x87D00324", "0x80072EE2", "0x800F081F", "0x80070002", "-2147024891"]


def words(r, n):
    return " ".join(r.choice(WORDS) for _ in range(n))


def cm_message(r):
    """A CMTrace message and its type (1 info, 2 warning, 3 error)."""
    k = r.random()
    guid = "%08X-%04X-%04X-%04X-%012X" % (r.getrandbits(32), r.getrandbits(16), r.getrandbits(16),
                                          r.getrandbits(16), r.getrandbits(48))
    if k < 0.04:
        return f"Failed to {words(r, 2)} with error {r.choice(ERRORS)}", 3
    if k < 0.05:
        return f"Unmatched exit code (1603) is considered an execution failure.", 3
    if k < 0.10:
        return f"Warning: {words(r, 2)} took {r.randint(1000, 90000)} ms, exceeding the threshold", 2
    if k < 0.12:
        return (f"Raising event:\ninstance of CCM_{words(r, 1).capitalize()}Event\n{{\n"
                f"    ClientID = \"GUID:{guid}\";\n    DateTime = \"20261023101010.000000+000\";\n}};"), 1
    if k < 0.30:
        return f"Successfully completed {words(r, 2)} for ScopeId_{guid[:8]}/Application_{guid[-8:].lower()}", 1
    if k < 0.40:
        return f"Download of {words(r, 1)} '{guid}' completed, {r.randint(1, 10_000_000)} bytes transferred", 1
    return f"{words(r, 1).capitalize()} {words(r, r.randint(3, 12))}", 1


def cm_line(msg, t, when, comp, thread):
    return (f'<![LOG[{msg}]LOG]!><time="{when:%H:%M:%S}.{when.microsecond // 1000:03d}-120" '
            f'date="{when:%m-%d-%Y}" component="{comp}" context="" type="{t}" thread="{thread}" '
            f'file="{comp.lower()}.cpp:{sum(map(ord, comp)) % 900 + 100}">')


def cmtrace_lines(r, count, start=DAY, step_ms=(0, 3000)):
    threads = [r.randint(100, 20000) for _ in range(12)]
    when = start
    for _ in range(count):
        when += dt.timedelta(milliseconds=r.randint(*step_ms))
        msg, t = cm_message(r)
        yield cm_line(msg, t, when, r.choice(COMPONENTS), r.choice(threads))


def write(path, lines, encoding="utf-8", newline="\r\n", final_newline=True):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding=encoding, newline="") as f:
        first = True
        for line in lines:
            if not first:
                f.write(newline)
            f.write(line.replace("\n", newline))
            first = False
        if final_newline:
            f.write(newline)


def write_big(path, line_gen, target_bytes):
    """Writes lines until the file has about `target_bytes` (UTF-8, CRLF)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    size = 0
    with open(path, "w", encoding="utf-8", newline="") as f:
        for line in line_gen:
            line = line.replace("\n", "\r\n") + "\r\n"
            f.write(line)
            size += len(line.encode("utf-8"))
            if size >= target_bytes:
                break


def set_age(path, minutes_ago):
    t = time.time() - minutes_ago * 60
    os.utime(path, (t, t))


# ----------------------------------------------------------------------
# CBS / DISM / IIS
# ----------------------------------------------------------------------

def cbs_lines(r, count, start=DAY):
    when = start
    for _ in range(count):
        when += dt.timedelta(milliseconds=r.randint(0, 1500))
        k = r.random()
        comp = "CSI" if r.random() < 0.3 else "CBS"
        if k < 0.03:
            yield (f"{when:%Y-%m-%d %H:%M:%S}, Error                 {comp}    Failed to {words(r, 2)} "
                   f"[HRESULT = {r.choice(ERRORS[:6])}]")
            if r.random() < 0.5:
                yield f"   Store corruption detected, manifest hash mismatch for {words(r, 1)}"
                yield f"   Component: Microsoft-Windows-{words(r, 1).capitalize()}, version 10.0.26100.{r.randint(1, 999)}"
        elif k < 0.08:
            yield f"{when:%Y-%m-%d %H:%M:%S}, Warning               {comp}    {words(r, 5)}"
        elif comp == "CSI":
            yield (f"{when:%Y-%m-%d %H:%M:%S}, Info                  CSI    {r.randint(1, 0xFFFF):08x} "
                   f"{words(r, 6)}")
        else:
            yield f"{when:%Y-%m-%d %H:%M:%S}, Info                  CBS    {words(r, r.randint(3, 10)).capitalize()}."


def dism_lines(r, count, start=DAY):
    when = start
    pid = 4312
    providers = ["DISM Provider Store", "DISM Package Manager", "DISM OS Provider", "DISM Manager"]
    funcs = ["CDISMManager::put_ScratchDir", "CDISMProviderStore::GetProvider",
             "CPackageManagerCLIHandler::Private_ProcessPackage", "CDISMPackageManager::Internal_Finalize"]
    yield f"{when:%Y-%m-%d %H:%M:%S}, Info                  DISM   DISM.EXE: <----- Starting Dism.exe session ----->"
    for _ in range(count):
        when += dt.timedelta(milliseconds=r.randint(0, 800))
        tid = r.choice([1188, 1204, 5532])
        k = r.random()
        level, text = ("Error", f"Failed {words(r, 2)}. hr:{r.choice(ERRORS[:6])}") if k < 0.04 else \
                      ("Warning", f"{words(r, 4).capitalize()}.") if k < 0.09 else \
                      ("Info", f"{words(r, r.randint(3, 9)).capitalize()}.")
        prov = f"{r.choice(providers)}: " if r.random() < 0.6 else ""
        func = f" - {r.choice(funcs)}" if r.random() < 0.8 else ""
        yield f"{when:%Y-%m-%d %H:%M:%S}, {level:<22}DISM   {prov}PID={pid} TID={tid} {text}{func}"


IIS_FIELDS = ("date time s-ip cs-method cs-uri-stem cs-uri-query s-port cs-username c-ip "
              "cs(User-Agent) cs(Referer) sc-status sc-substatus sc-win32-status time-taken")
IIS_FIELDS_SHORT = "date time cs-method cs-uri-stem sc-status time-taken"


def iis_lines(r, count, start=DAY, header=True, fields=IIS_FIELDS):
    if header:
        yield "#Software: Microsoft Internet Information Services 10.0"
        yield "#Version: 1.0"
        yield f"#Date: {start:%Y-%m-%d %H:%M:%S}"
        yield f"#Fields: {fields}"
    when = start
    uris = ["/default.aspx", "/api/orders", "/api/customers", "/login", "/static/app.js", "/favicon.ico"]
    for _ in range(count):
        when += dt.timedelta(milliseconds=r.randint(0, 2000))
        k = r.random()
        status = 500 if k < 0.03 else 404 if k < 0.08 else 401 if k < 0.10 else 304 if k < 0.2 else 200
        method = "POST" if r.random() < 0.2 else "GET"
        uri = r.choice(uris)
        query = f"id={r.randint(1, 9999)}" if r.random() < 0.3 else "-"
        user = r.choice(["-", "-", "CONTOSO\\jdoe", "CONTOSO\\asmith"])
        client = f"10.0.{r.randint(0, 3)}.{r.randint(2, 250)}"
        if fields == IIS_FIELDS_SHORT:
            yield f"{when:%Y-%m-%d %H:%M:%S} {method} {uri} {status} {r.randint(1, 3000)}"
        else:
            yield (f"{when:%Y-%m-%d %H:%M:%S} 10.0.0.5 {method} {uri} {query} 443 {user} {client} "
                   f"Mozilla/5.0+(Windows+NT+10.0;+Win64;+x64) - {status} 0 {64 if status == 500 else 0} "
                   f"{r.randint(1, 3000)}")


EXCH_FIELDS = "date-time,session,source,Destination,direction,description"


def exchange_connect_lines(r, count, start=DAY):
    """Exchange Transport Connectivity log: comma-separated W3C-like format, UTC "T...Z" times,
    direction + - > < *, some descriptions quoted because they contain commas."""
    yield "#Software: Microsoft Exchange Server"
    yield "#Version: 15.0.0.0"
    yield "#Log-type: Transport Connectivity Log"
    yield f"#Date: {start:%Y-%m-%dT%H:%M:%S}.000Z"
    yield f"#Fields: {EXCH_FIELDS}"
    when = start
    hosts = ["mail.contoso.com", "smtp.fabrikam.com", "shadowredundancy", "mx.example.org"]
    for _ in range(count):
        when += dt.timedelta(milliseconds=r.randint(0, 3000))
        session = f"08DF1042B9A7{r.randint(0, 0xFFFF):04X}"
        host = r.choice(hosts)
        k = r.random()
        if k < 0.05:
            d, desc = ">", f'"Failed connection to 10.0.{r.randint(0, 3)}.{r.randint(2, 250)}:25 (TimedOut:10060, Socket error)"'
        elif k < 0.10:
            d, desc = "*", "Retrying the connection attempt"
        elif k < 0.35:
            d, desc = "+", f"{host} [10.0.0.{r.randint(2, 250)}] connecting"
        elif k < 0.60:
            d, desc = "-", f"Messages: {r.randint(0, 20)} Bytes: {r.randint(0, 10**6)} (Session closed)"
        elif k < 0.80:
            d, desc = ">", f"Established connection to 10.0.0.{r.randint(2, 250)}"
        else:
            d, desc = "+", "ShadowRedundancy 00000000-0000-0000-0000-000000000000;QueueLength=<no priority counts>. "
        yield f"{when:%Y-%m-%dT%H:%M:%S}.{when.microsecond // 1000:03d}Z,{session},SMTP,{host},{d},{desc}"


# ----------------------------------------------------------------------

def small(r):
    # CMTrace in several encodings
    write(ROOT / "cmtrace" / "AppEnforce.log", cmtrace_lines(r, 3000))
    write(ROOT / "cmtrace" / "utf8_bom.log", cmtrace_lines(r, 200), encoding="utf-8-sig")
    write(ROOT / "cmtrace" / "utf16le.log",
          (cm_line(m, t, DAY + dt.timedelta(seconds=i), "Inštalácia", 7) for i, (m, t) in enumerate(
              [("Príliš žltý kôň úpel ďábelské ódy", 1), ("Chyba pri inštalácii balíka", 3),
               ("Varovanie: málo miesta na disku", 2), ("Grüße aus München – Größe 1 MB", 1)] * 25)),
          encoding="utf-16")
    write(ROOT / "cmtrace" / "incomplete_last_line.log",
          list(cmtrace_lines(r, 20)) + ['<![LOG[Message without end of line'], final_newline=False)
    write(ROOT / "cmtrace" / "multiline.log", [
        cm_line("Single line entry", 1, DAY, "Test", 1),
        cm_line("First line\nsecond line\n    indented third line", 1, DAY + dt.timedelta(seconds=1), "Test", 1),
        cm_line("Exit code 1603 means a fatal error\nduring installation", 3, DAY + dt.timedelta(seconds=2), "Test", 1),
        cm_line("Last entry", 2, DAY + dt.timedelta(seconds=3), "Test", 1),
    ])
    # Old SMS format and plain text
    sms = []
    when = DAY
    for i in range(300):
        when += dt.timedelta(seconds=r.randint(0, 20))
        m, t = cm_message(r)
        m = m.split("\n")[0]
        th = r.choice([4420, 5120, 1880])
        sms.append(f"{m}  $$<SMS_{r.choice(['EXECUTIVE', 'DISTRIBUTION_MANAGER', 'POLICY_PROVIDER'])}>"
                   f"<{when:%m-%d-%Y %H:%M:%S}.{r.randint(0, 999):03d}-120><thread={th} (0x{th:X})>")
    write(ROOT / "other" / "sms_old_format.log", sms)
    write(ROOT / "other" / "plain_ansi1250.log",
          [f"{DAY + dt.timedelta(minutes=i):%Y-%m-%d %H:%M:%S} " + r.choice(
              ["Spúšťam službu", "Chyba: súbor nenájdený", "Varovanie: žiadna odpoveď", "Úloha dokončená",
               "warning: retry", "FAILED to connect"]) for i in range(500)], encoding="cp1250")
    write(ROOT / "other" / "plain_no_timestamps.log",
          [r.choice(["Starting", "error occurred", "warning issued", "done", words(r, 6)]) for _ in range(400)])
    # Formats with log definitions
    write(ROOT / "cbs" / "CBS.log", cbs_lines(r, 3000))
    write(ROOT / "dism" / "dism.log", dism_lines(r, 3000))
    write(ROOT / "iis" / "u_ex261023.log",
          list(iis_lines(r, 1500)) + list(iis_lines(r, 500, DAY + dt.timedelta(hours=2), fields=IIS_FIELDS_SHORT)))
    write(ROOT / "iis" / "webserver-access.txt", iis_lines(r, 300))  # detected by content, not by name
    write(ROOT / "exchange" / "CONNECTLOG2026102300-1.LOG", exchange_connect_lines(r, 2000))
    write(ROOT / "exchange" / "transport-connectivity.txt", exchange_connect_lines(r, 300))  # by content
    # Rotation: service.log + older parts (by modification time)
    rot = ROOT / "rotation"
    parts = [("service-20261023-080000.log", 180), ("service-20261023-090000.log", 120),
             ("service.lo_", 60), ("service.log", 0)]
    start = DAY + dt.timedelta(hours=7)
    for name, age in parts:
        write(rot / name, cmtrace_lines(r, 400, start))
        set_age(rot / name, age)
        start += dt.timedelta(hours=1)


def big(r):
    print("big_cmtrace_400mb.log ...", flush=True)
    write_big(ROOT / "big" / "big_cmtrace_400mb.log", cmtrace_lines(r, 10**9, step_ms=(0, 60)), 400 * 2**20)
    print("cbs_big_100mb.log ...", flush=True)
    write_big(ROOT / "big" / "cbs_big_100mb.log", cbs_lines(r, 10**9), 100 * 2**20)
    print("u_ex_big_100mb.log ...", flush=True)
    write_big(ROOT / "big" / "u_ex_big_100mb.log", iis_lines(r, 10**9), 100 * 2**20)


README = """RusTrace test logs – generated by scripts/gen_testlogs.py (not in git; run it again to recreate).

cmtrace/AppEnforce.log            CMTrace, 3000 entries, info/warning/error, error codes, multi-line entries
cmtrace/utf8_bom.log              CMTrace in UTF-8 with BOM
cmtrace/utf16le.log               CMTrace in UTF-16 LE (BOM), Slovak/German characters
cmtrace/incomplete_last_line.log  last entry unfinished, no final line break (provisional entry)
cmtrace/multiline.log             multi-line CMTrace messages
other/sms_old_format.log          old SMS format  msg $$<component><date time><thread=...>
other/plain_ansi1250.log          plain text in Windows-1250 (ANSI), keyword severity
other/plain_no_timestamps.log     plain text without any date/time (time filter notice)
cbs/CBS.log                       CBS log definition (continuation lines, Info/Warning/Error)
dism/dism.log                     DISM log definition (provider, PID/TID, function)
iis/u_ex261023.log                IIS W3C, a second #Fields header with fewer columns in the middle
iis/webserver-access.txt          IIS W3C detected by content (the name matches no definition)
exchange/CONNECTLOG2026102300-1.LOG  Exchange Transport Connectivity log (comma-separated W3C, quoted values)
exchange/transport-connectivity.txt  the same format detected by content
rotation/service.log              active log; service.lo_ and service-20261023-0X0000.log are older parts
                                  (merged when service.log is opened; modification times set in order)
big/big_cmtrace_400mb.log         400 MB CMTrace (memory / speed measurements)
big/cbs_big_100mb.log             100 MB CBS log
big/u_ex_big_100mb.log            100 MB IIS log
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--small-only", action="store_true", help="do not create the big logs")
    args = ap.parse_args()
    r = random.Random(20261023)
    ROOT.mkdir(exist_ok=True)
    small(r)
    if not args.small_only:
        big(r)
    (ROOT / "README.txt").write_text(README, encoding="utf-8")
    print(f"done: {ROOT}")


if __name__ == "__main__":
    main()
