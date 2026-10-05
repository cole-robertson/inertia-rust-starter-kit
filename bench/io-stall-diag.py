#!/usr/bin/env python3
"""Timestamp the multi-second write stalls of the Rust image and see what lines up with them.

Runs the irsk:io container (pinned 0-3, pool 10, DB_CONNECT_TIMEOUT=10000 so a stall shows as
latency, not a 500) with its storage on STORAGE (a host dir), then:
  - 32 async writers POST /bench/write for DURATION s, recording (start, latency, status) of
    every request over 100 ms;
  - a sampler every 20 ms: WAL file size, and the state of every app thread (/proc/<pid>/task/*/stat
    field 3 + wchan), noting threads in D (uninterruptible I/O) state.
Prints each stall with what the sampler saw around it.
  STORAGE=~/.cache/stall DURATION=30 python3 bench/io-stall-diag.py
  MEM=32g ...          container memory limit (default 4g, as bench/io-run.sh)
  CKPT=1 ...           also run `PRAGMA wal_checkpoint(TRUNCATE)` every CKPT seconds
  NOCOW=1 ...          chattr +C the storage dir first (btrfs: no copy-on-write)
Needs the irsk:io image and passwordless `sudo docker`.
"""
import asyncio, json, os, subprocess, sys, time, threading, glob

STORAGE = os.path.expanduser(os.environ.get("STORAGE", "~/.cache/stall-btrfs"))
DURATION = float(os.environ.get("DURATION", "60"))
EXTRA = os.environ.get("EXTRA_ENV", "").split()
PORT = 5410
BODY = json.dumps({"payload": "p" * 200}).encode()
D = ["sudo", "-n", "docker"]

subprocess.run(D + ["rm", "-f", "stall-app"], capture_output=True)
subprocess.run(["sudo", "-n", "rm", "-rf", STORAGE]); os.makedirs(STORAGE); os.chmod(STORAGE, 0o777)
if os.environ.get("NOCOW"): subprocess.run(["chattr", "+C", STORAGE], check=True)
env = ["-e", f"PORT={PORT}", "-e", "BINDING=127.0.0.1", "-e", "SECRET_KEY_BASE=" + "a" * 128,
       "-e", "HOST=https://bench.local", "-e", "ALLOW_INSECURE_HTTP=true", "-e", "MAILER_HOST=localhost",
       "-e", "MAILER_USER=x", "-e", "MAILER_PASSWORD=x", "-e", "COMPRESSION=false", "-e", "LOG_LEVEL=warn",
       "-e", "DB_CONNECT_TIMEOUT=10000"]
for e in EXTRA:
    env += ["-e", e]
subprocess.run(D + ["run", "-d", "--name", "stall-app", "--network", "host", "--cpuset-cpus", "0-3",
                    "--memory", os.environ.get("MEM","4g"), "-v", f"{STORAGE}:/app/storage"] + env + ["irsk:io"], check=True,
               capture_output=True)
pid = int(subprocess.run(D + ["inspect", "-f", "{{.State.Pid}}", "stall-app"], capture_output=True,
                         text=True).stdout)
for _ in range(200):
    try:
        import urllib.request
        urllib.request.urlopen(f"http://127.0.0.1:{PORT}/up", timeout=1); break
    except Exception:
        time.sleep(0.05)

# The app process: the container's init is tini; find the -cli child.
app = None
for _ in range(50):
    out = subprocess.run(["pgrep", "-f", "inertia_rust_starter_kit-cli start"], capture_output=True, text=True).stdout.split()
    if out:
        app = int(out[0]); break
    time.sleep(0.1)
wal = os.path.join(STORAGE, "production.sqlite-wal")
samples = []  # (t, wal_bytes, [(tid, comm, state, wchan)] for non-S/R... we keep D and R counts)
stop = False


def sampler():
    while not stop:
        t = time.time()
        try:
            w = os.path.getsize(wal)
        except OSError:
            w = -1
        d_threads = []
        for st in glob.glob(f"/proc/{app}/task/*/stat"):
            try:
                f = open(st).read()
                comm = f[f.index("(") + 1:f.rindex(")")]
                state = f[f.rindex(")") + 2]
                if True:
                    tid = st.split("/")[4]
                    try:
                        wc = open(f"/proc/{app}/task/{tid}/wchan").read()
                    except OSError:
                        wc = "?"
                    if wc not in ("0", "", "do_epoll_wait", "futex_wait_queue", "futex_do_wait", "ep_poll", "?") : d_threads.append(f"{comm}:{state}:{wc}")
            except (OSError, ValueError):
                pass
        samples.append((t, w, d_threads))
        time.sleep(0.02)


threading.Thread(target=sampler, daemon=True).start()
ckpt = []
def checkpointer():
    import sqlite3
    while not stop:
        time.sleep(float(os.environ.get("CKPT", "0")) or 3600)
        if stop or not os.environ.get("CKPT"): continue
        try:
            c = sqlite3.connect(os.path.join(STORAGE, "production.sqlite"), timeout=5)
            s0 = time.time(); r = c.execute("pragma wal_checkpoint(TRUNCATE)").fetchone(); c.close()
            ckpt.append((round(time.time() - s0, 3), r))
        except Exception as e:
            ckpt.append(("err", str(e)))
threading.Thread(target=checkpointer, daemon=True).start()
slow = []
n = {"ok": 0, "err": 0}


async def writer(deadline):
    while time.time() < deadline:
        t0 = time.time()
        r, w = await asyncio.open_connection("127.0.0.1", PORT)
        w.write(b"POST /bench/write HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n"
                b"User-Agent: Mozilla/5.0 Chrome/140.0\r\nConnection: close\r\nContent-Length: %d\r\n\r\n" % len(BODY) + BODY)
        await w.drain()
        line = await r.readline()
        await r.read()
        w.close()
        dt = time.time() - t0
        code = line.split()[1].decode() if len(line.split()) > 1 else "?"
        n["ok" if code == "200" else "err"] += 1
        if dt > 0.1:
            slow.append((t0, dt, code))


async def main():
    deadline = time.time() + DURATION
    await asyncio.gather(*(writer(deadline) for _ in range(32)))

cid = subprocess.run(D + ["inspect", "-f", "{{.Id}}", "stall-app"], capture_output=True, text=True).stdout.strip()
cgp = glob.glob(f"/sys/fs/cgroup/**/*{cid}*/memory.events", recursive=True)
def ev():
    if not cgp: return {}
    d = dict(l.split() for l in open(cgp[0]))
    st = dict(l.split() for l in open(cgp[0].replace("memory.events", "memory.stat")))
    pr = open(cgp[0].replace("memory.events", "memory.pressure")).read().split()[4]
    return {"high": d.get("high"), "max": d.get("max"), "file_dirty": st.get("file_dirty"), "file_writeback": st.get("file_writeback"), "file": st.get("file"), "pressure_full_total": pr}
ev0 = ev()
t_start = time.time()
asyncio.run(main())
ev1 = ev()
stop = True
time.sleep(0.1)
logs = subprocess.run(D + ["logs", "stall-app"], capture_output=True, text=True)
subprocess.run(D + ["rm", "-f", "stall-app"], capture_output=True)

print(f"storage={STORAGE} fs={subprocess.run(['findmnt','-no','FSTYPE','-T',STORAGE],capture_output=True,text=True).stdout.strip()} "
      f"extra={EXTRA} requests ok={n['ok']} err={n['err']} rate={n['ok']/DURATION:.0f}/s slow(>100ms)={len(slow)}")
# Group slow requests into stall episodes (starts within 1 s of each other).
slow.sort()
episodes = []
for t0, dt, code in slow:
    if episodes and t0 - episodes[-1]["start"] < 1.0:
        e = episodes[-1]; e["n"] += 1; e["max"] = max(e["max"], dt)
    else:
        episodes.append({"start": t0, "n": 1, "max": dt})
for e in episodes:
    win = [s for s in samples if e["start"] - 0.2 <= s[0] <= e["start"] + e["max"] + 0.2]
    wals = [s[1] for s in win]
    dset = sorted({d for s in win for d in s[2]})
    walchg = f"{min(wals)//1024}k..{max(wals)//1024}k" if wals else "?"
    before = [s[1] for s in samples if e["start"] - 1.0 <= s[0] < e["start"]]
    print(f"  t+{e['start']-t_start:6.2f}s  {e['n']:3d} slow, max {e['max']*1000:6.0f} ms  WAL {walchg}"
          f" (1 s before: {max(before)//1024 if before else '?'}k)  D-state: {dset[:4]}")
walmax = max(s[1] for s in samples) // 1024
resets = sum(1 for a, b in zip(samples, samples[1:]) if b[1] < a[1])
print(f"  checkpoints: {len(ckpt)}, first {ckpt[:3]}, busy={sum(1 for c in ckpt if c[0] != chr(101)+chr(114)+chr(114) and c[1][0] == 1)}")
print(f"  cgroup before: {ev0}\n  cgroup after:  {ev1}")
print(f"  WAL max {walmax}k, shrank {resets} times; app warn lines: {len(logs.stdout.splitlines())}")
