import resource, subprocess, sys, time
t = time.time(); out = subprocess.run(sys.argv[1:], capture_output=True, text=True).stdout
print(f"{time.time()-t:.2f} s wall, {resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss} KiB max RSS")
print("\n".join(out.splitlines()[6:9]))
