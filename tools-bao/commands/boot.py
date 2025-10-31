import sys
import time
import logging
import serial

def cmd_boot(args) -> None:
    port = args.port
    baud = args.baud
    try:
        ser = serial.Serial(port, baud, timeout=0.2)
    except Exception as e:
        logging.error(f"[bao] cannot open {port}: {e}")
        sys.exit(2)

    try:
        with ser:
            try:
                ser.reset_input_buffer()
                ser.reset_output_buffer()
            except Exception:
                pass

            # Send the boot command to leave bootloader mode and start firmware (run mode)
            ser.write(b"boot\r\n")
            ser.flush()
            # tiny grace period to ensure the device processes it
            time.sleep(0.1)
    except Exception as e:
        logging.error(f"[bao] boot command failed on {port}: {e}")
        sys.exit(1)

    print(f"[bao] sent 'boot' on {port}")
    sys.exit(0)
