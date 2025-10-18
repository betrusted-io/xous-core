from serial.tools import list_ports

def cmd_doctor(_args):
    """Quick environment check."""
    try:
        ports = list(list_ports.comports())
        print(f"[bao] Python OK; pyserial OK; ports found: {len(ports)}")
        for p in ports:
            print(f" - {p.device} {p.description}")
    except Exception as e:
        print(f"[bao] Doctor failed: {e}")
