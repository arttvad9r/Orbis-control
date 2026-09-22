import subprocess
r = subprocess.run(["convert", "--version"], capture_output=True, text=True)
print(r.stdout.splitlines()[0] if r.stdout else r.stderr.splitlines()[0])
r2 = subprocess.run(["identify", "--version"], capture_output=True, text=True)
print(r2.stdout.splitlines()[0] if r2.stdout else r2.stderr.splitlines()[0])
