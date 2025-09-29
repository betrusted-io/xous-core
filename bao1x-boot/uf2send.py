#!/usr/bin/env python3

import argparse
import base64
import sys
import time
import threading
import re
import struct
from pathlib import Path
from typing import Optional, Tuple, List, Dict
from dataclasses import dataclass
from queue import Queue, Empty

import serial
from serial.tools import list_ports
from progressbar.bar import ProgressBar
import progressbar


# Configuration constants
RETRY_TIMES = 3  # Number of retries per block
RETRY_LIMIT = 5  # Maximum failed blocks before quitting
RESPONSE_TIMEOUT = 0.5  # 500ms timeout for device response


@dataclass
class BlockInfo:
    """Information about a UF2 block."""
    block_num: int
    address: int
    payload_size: int
    data: bytes


@dataclass
class TransferResult:
    """Result of a block transfer attempt."""
    success: bool
    attempts: int
    error_msg: Optional[str] = None


def list_available_ports() -> list[str]:
    """List all available serial ports."""
    return [port.device for port in list_ports.comports()]


def parse_uf2_block(block: bytes) -> Optional[BlockInfo]:
    """
    Parse a UF2 block to extract address and payload information.
    
    UF2 block structure (512 bytes):
    - 0-3: Magic start (0x0A324655)
    - 4-7: Magic start (0x9E5D5157)
    - 8-11: Flags
    - 12-15: Target address
    - 16-19: Payload size
    - 20-23: Block number
    - 24-27: Total blocks
    - 28-31: Family ID (optional)
    - 32-475: Data (up to 476 bytes)
    - 476-507: Unused
    - 508-511: Magic end (0x0AB16F30)
    """
    if len(block) != 512:
        return None
    
    # Check magic numbers
    magic_start = struct.unpack('<I', block[0:4])[0]
    magic_end = struct.unpack('<I', block[508:512])[0]
    
    if magic_start != 0x0A324655 or magic_end != 0x0AB16F30:
        return None
    
    # Extract block information
    target_addr = struct.unpack('<I', block[12:16])[0]
    payload_size = struct.unpack('<I', block[16:20])[0]
    block_num = struct.unpack('<I', block[20:24])[0]
    
    # Extract actual data
    data = block[32:32+payload_size]
    
    return BlockInfo(
        block_num=block_num,
        address=target_addr,
        payload_size=payload_size,
        data=data
    )


def wait_for_response(
    ser: serial.Serial,
    expected_size: int,
    expected_addr: int,
    timeout: float = RESPONSE_TIMEOUT,
    verbose: bool = False
) -> Tuple[bool, str]:
    """
    Wait for and validate device response.
    
    Returns:
        Tuple of (success, error_message)
    """
    start_time = time.time()
    buffer = b''
    
    while time.time() - start_time < timeout:
        if ser.in_waiting > 0:
            buffer += ser.read(ser.in_waiting)
            
            # Try to decode and find response pattern
            try:
                text = buffer.decode('utf-8', errors='ignore')
                lines = text.split('\n')
                
                for line in lines:
                    # Look for pattern: "Wrote 256 to 0x60070a00"
                    match = re.search(r'Wrote\s+(\d+)\s+to\s+(0x[0-9a-fA-F]+)', line)
                    if match:
                        recv_size = int(match.group(1))
                        recv_addr = int(match.group(2), 16)
                        
                        if verbose:
                            print(f"\n[DEBUG] Received: size={recv_size}, addr=0x{recv_addr:08x}")
                            print(f"[DEBUG] Expected: size={expected_size}, addr=0x{expected_addr:08x}")
                        
                        if recv_size == expected_size and recv_addr == expected_addr:
                            return True, ""
                        else:
                            return False, f"Mismatch: expected {expected_size} bytes @ 0x{expected_addr:08x}, got {recv_size} bytes @ 0x{recv_addr:08x}"
                
            except Exception as e:
                if verbose:
                    print(f"[DEBUG] Parse error: {e}")
        
        time.sleep(0.01)  # Small sleep to prevent busy waiting
    
    return False, f"Timeout ({timeout}s) - no valid response received"


def send_uf2_block(
    ser: serial.Serial,
    block: bytes,
    block_info: BlockInfo,
    retry_times: int = RETRY_TIMES,
    verbose: bool = False
) -> TransferResult:
    """
    Send a single UF2 block with retry logic.
    
    Returns:
        TransferResult with success status and attempt count
    """
    encoded = base64.b64encode(block).decode('ascii')
    message = f"uf2 {encoded}\r"
    
    for attempt in range(1, retry_times + 1):
        try:
            # Clear input buffer before sending
            ser.reset_input_buffer()
            
            # Send the block
            ser.write(message.encode('utf-8'))
            ser.flush()
            
            # Wait for response
            success, error_msg = wait_for_response(
                ser,
                block_info.payload_size,
                block_info.address,
                timeout=RESPONSE_TIMEOUT,
                verbose=verbose
            )
            
            if success:
                return TransferResult(success=True, attempts=attempt)
            
            if attempt < retry_times:
                if verbose:
                    print(f"\n[RETRY] Block {block_info.block_num}: {error_msg}")
                time.sleep(0.1)  # Brief pause before retry
            
        except serial.SerialTimeoutException as e:
            error_msg = f"Write timeout: {e}"
            if attempt == retry_times:
                return TransferResult(success=False, attempts=attempt, error_msg=error_msg)
        except Exception as e:
            error_msg = f"Unexpected error: {e}"
            if attempt == retry_times:
                return TransferResult(success=False, attempts=attempt, error_msg=error_msg)
    
    return TransferResult(success=False, attempts=retry_times, error_msg=error_msg)


def send_uf2_file(
    uf2_path: Path,
    port: str,
    baudrate: int = 1_000_000,
    retry_limit: int = RETRY_LIMIT,
    verbose: bool = False,
    timeout: float = 1.0
) -> None:
    """
    Send UF2 file with response validation and retry logic.
    """
    if not uf2_path.exists():
        raise FileNotFoundError(f"UF2 file not found: {uf2_path}")
    
    if not uf2_path.is_file():
        raise ValueError(f"Path is not a file: {uf2_path}")
    
    # Open serial port
    try:
        ser = serial.Serial(
            port=port,
            baudrate=baudrate,
            timeout=timeout,
            write_timeout=timeout
        )
        ser.reset_input_buffer()
        ser.reset_output_buffer()
    except serial.SerialException as e:
        raise ConnectionError(f"Failed to open serial port {port}: {e}")
    
    # Read file and count blocks
    file_size = uf2_path.stat().st_size
    if file_size % 512 != 0:
        ser.close()
        raise ValueError(f"Invalid UF2 file size: {file_size} (not multiple of 512)")
    
    total_blocks = file_size // 512
    
    print(f"\n{'='*60}")
    print(f"UF2 Transfer Starting")
    print(f"{'='*60}")
    print(f"File: {uf2_path.name}")
    print(f"Port: {port} @ {baudrate} baud")
    print(f"Total blocks: {total_blocks}")
    print(f"Retry limit: {RETRY_TIMES} attempts per block")
    print(f"Failure threshold: {retry_limit} blocks")
    print(f"{'='*60}\n")
    
    # Initialize progress bar
    widgets = [
        'Transfer: ',
        progressbar.Percentage(),
        ' ',
        progressbar.Bar(marker='█', left='[', right=']'),
        ' ',
        progressbar.ETA(),
        ' | ',
        progressbar.Variable('status', format='{formatted_value}'),
    ]

    pbar = ProgressBar(
        widgets=widgets,
        max_value=total_blocks,
        redirect_stdout=True,
        redirect_stderr=True
    )
    
    # Track failures
    failed_blocks: Dict[int, TransferResult] = {}
    total_retries = 0
    
    try:
        with uf2_path.open('rb') as f:
            # Send twice because the very first message is sometimes messed up in serial protocol
            ser.write("localecho off\r".encode('utf-8'))
            ser.flush()
            ser.write("localecho off\r".encode('utf-8'))
            ser.flush()
            time.sleep(0.1)

            for block_idx in range(total_blocks):
                # Read and parse block
                block = f.read(512)
                block_info = parse_uf2_block(block)
                
                if not block_info:
                    print(f"\n[ERROR] Invalid UF2 block at position {block_idx}")
                    failed_blocks[block_idx] = TransferResult(
                        success=False, 
                        attempts=0, 
                        error_msg="Invalid UF2 block format"
                    )
                    
                    if len(failed_blocks) >= retry_limit:
                        break
                    continue
                
                # Send block with retry logic
                result = send_uf2_block(
                    ser, block, block_info, 
                    retry_times=RETRY_TIMES, 
                    verbose=verbose
                )
                
                if not result.success:
                    failed_blocks[block_idx] = result
                    status = f"Failed: {len(failed_blocks)}"
                    
                    if len(failed_blocks) >= retry_limit:
                        pbar.update(block_idx + 1, status=status)
                        break
                else:
                    if result.attempts > 1:
                        total_retries += result.attempts - 1
                    status = f"OK (Retries: {total_retries})"
                
                pbar.update(block_idx + 1, status=status)
        
        pbar.finish()
        
    finally:
        ser.write("localecho on\r".encode('utf-8'))
        ser.flush()
        ser.close()
    
    # Print summary
    print(f"\n{'='*60}")
    print(f"Transfer Summary")
    print(f"{'='*60}")
    
    successful_blocks = total_blocks - len(failed_blocks)
    success_rate = (successful_blocks / total_blocks * 100) if total_blocks > 0 else 0
    
    print(f"Total blocks: {total_blocks}")
    print(f"Successful: {successful_blocks} ({success_rate:.1f}%)")
    print(f"Failed: {len(failed_blocks)}")
    print(f"Total retries: {total_retries}")
    
    if failed_blocks:
        print(f"\n{'='*60}")
        print(f"Failed Blocks Details")
        print(f"{'='*60}")
        
        for block_idx, result in sorted(failed_blocks.items())[:10]:  # Show first 10 failures
            print(f"Block {block_idx}: {result.attempts} attempts - {result.error_msg}")
        
        if len(failed_blocks) > 10:
            print(f"... and {len(failed_blocks) - 10} more failures")
        
        if len(failed_blocks) >= retry_limit:
            print(f"\n[ABORTED] Transfer stopped - exceeded failure limit ({retry_limit} blocks)")
            sys.exit(1)
    else:
        print(f"\n[SUCCESS] All blocks transferred successfully!")
    
    print(f"{'='*60}\n")


def main():
    global RETRY_TIMES, RETRY_LIMIT
    parser = argparse.ArgumentParser(
        description='Send UF2 file over serial port with response validation and retry logic',
        formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    
    parser.add_argument(
        'uf2_file',
        type=Path,
        help='Path to the UF2 file to send'
    )
    
    parser.add_argument(
        '-p', '--port',
        type=str,
        default='/dev/ttyACM0' if sys.platform != 'win32' else 'COM3',
        help='Serial port device path'
    )
    
    parser.add_argument(
        '-b', '--baudrate',
        type=int,
        default=1_000_000,
        help='Serial port baud rate'
    )
    
    parser.add_argument(
        '-r', '--retry-times',
        type=int,
        default=RETRY_TIMES,
        help='Number of retries per block on failure'
    )
    
    parser.add_argument(
        '-f', '--failure-limit',
        type=int,
        default=RETRY_LIMIT,
        help='Maximum failed blocks before aborting'
    )
    
    parser.add_argument(
        '-t', '--timeout',
        type=float,
        default=1.0,
        help='Serial port timeout in seconds'
    )
    
    parser.add_argument(
        '-l', '--list-ports',
        action='store_true',
        help='List available serial ports and exit'
    )
    
    parser.add_argument(
        '-v', '--verbose',
        action='store_true',
        help='Enable verbose debug output'
    )
    
    args = parser.parse_args()
    
    # Update global constants if provided
    RETRY_TIMES = args.retry_times
    RETRY_LIMIT = args.failure_limit
    
    # List ports if requested
    if args.list_ports:
        ports = list_available_ports()
        if ports:
            print("Available serial ports:")
            for port in ports:
                print(f"  - {port}")
        else:
            print("No serial ports found")
        return 0
    
    # Send file
    try:
        send_uf2_file(
            uf2_path=args.uf2_file,
            port=args.port,
            baudrate=args.baudrate,
            retry_limit=args.failure_limit,
            verbose=args.verbose,
            timeout=args.timeout
        )
        return 0
        
    except (FileNotFoundError, ValueError) as e:
        print(f"[ERROR] {e}", file=sys.stderr)
        return 1
    except ConnectionError as e:
        print(f"[ERROR] Serial port: {e}", file=sys.stderr)
        print(f"Try listing available ports with: {sys.argv[0]} --list-ports")
        return 2
    except KeyboardInterrupt:
        print("\n[INTERRUPTED] Transfer cancelled by user")
        return 130
    except Exception as e:
        print(f"[ERROR] Unexpected: {e}", file=sys.stderr)
        if args.verbose:
            import traceback
            traceback.print_exc()
        return 3


if __name__ == '__main__':
    sys.exit(main())
