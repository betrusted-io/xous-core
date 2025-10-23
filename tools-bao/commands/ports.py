import logging
from serial.tools import list_ports

def cmd_ports(args) -> None:
    ports = list(list_ports.comports())
    if getattr(args, "verbose", False):
        logging.info(f"[bao] pyserial found {len(ports)} port(s)")
    if not ports:
        logging.warning("[bao] No serial ports found.")
        print("      Try: python -m serial.tools.list_ports -v")
        return
    for p in ports:
        vidpid = ""
        if p.vid is not None and p.pid is not None:
            vidpid = f" (VID:PID={p.vid:04x}:{p.pid:04x})"
        print(f"{p.device}\t{p.description}{vidpid}")
