//
// Copyright (c) 2010-2022 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.IO;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Exceptions;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Memory;
using Antmicro.Renode.Peripherals.SPI.NORFlash;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    public class MXIC_MX66UM1G45G : ISPIPeripheral
    {
        public MXIC_MX66UM1G45G(MappedMemory underlyingMemory)
        {
            // original MT25Q supports capacity 8MB to 256MB,
            // but we extended it down to 64KB
            // to become compatible with N25Q line
            if (underlyingMemory.Size < 64.KB() || underlyingMemory.Size > 256.MB() || !Misc.IsPowerOfTwo((ulong)underlyingMemory.Size))
            {
                throw new ConstructionException("Size of the underlying memory must be a power of 2 value in range 64KB - 256MB");
            }

            volatileConfigurationRegister = new ByteRegister(this, 0xfb).WithFlag(3, name: "XIP");
            nonVolatileConfigurationRegister = new WordRegister(this, 0xffff).WithFlag(0, out numberOfAddressBytes, name: "addressWith3Bytes");
            enhancedVolatileConfigurationRegister = new ByteRegister(this, 0xff)
                .WithValueField(0, 3, name: "Output driver strength")
                .WithReservedBits(3, 1)
                .WithTaggedFlag("Reset/hold", 4)
                //these flags are intentionally not implemented, as they described physical details
                .WithFlag(5, name: "Double transfer rate protocol")
                .WithFlag(6, name: "Dual I/O protocol")
                .WithFlag(7, name: "Quad I/O protocol");
            statusRegister = new ByteRegister(this).WithFlag(1, out enable, name: "writeEnableLatch");
            flagStatusRegister = new ByteRegister(this)
                .WithFlag(0, FieldMode.Read, valueProviderCallback: _ => numberOfAddressBytes.Value, name: "Addressing")
                //other bits indicate either protection errors (not implemented) or pending operations (they already finished)
                .WithReservedBits(3, 1)
                .WithFlag(7, FieldMode.Read, valueProviderCallback: _ => true, name: "ProgramOrErase");

            this.underlyingMemory = underlyingMemory;
            underlyingMemory.ResetByte = EmptySegment;

            deviceData = GetDeviceData();
        }

        public void FinishTransmission()
        {
            this.Log(LogLevel.Noisy, "Transmission finished");
            switch (currentOperation.State)
            {
                case DecodedOperation.OperationState.RecognizeOperation:
                case DecodedOperation.OperationState.AccumulateCommandAddressBytes:
                case DecodedOperation.OperationState.AccumulateNoDataCommandAddressBytes:
                    this.Log(LogLevel.Warning, "Transmission finished in the unexpected state: {0}", currentOperation.State);
                    break;
            }
            // If an operation has at least 1 data byte or more than 0 address bytes,
            // we can clear the write enable flag only when we are finishing a transmission.
            switch (currentOperation.Operation)
            {
                case DecodedOperation.OperationType.Program:
                case DecodedOperation.OperationType.Erase:
                case DecodedOperation.OperationType.WriteRegister:
                    //although the docs are not clear, it seems that all register writes should clear the flag
                    enable.Value = false;
                    break;
            }
            currentOperation.State = DecodedOperation.OperationState.RecognizeOperation;
            currentOperation = default(DecodedOperation);
            temporaryNonVolatileConfiguration = 0;
            isFirstOpiByte = true;
        }

        public void Reset()
        {
            this.Log(LogLevel.Noisy, "Resetting SPI flash device. Exiting OPI mode.");
            opiMode = false;
            isFirstOpiByte = true;
            statusRegister.Reset();
            flagStatusRegister.Reset();
            volatileConfigurationRegister.Reset();
            nonVolatileConfigurationRegister.Reset();
            enhancedVolatileConfigurationRegister.Reset();
            currentOperation = default(DecodedOperation);
            FinishTransmission();
        }

        public byte Transmit(byte data)
        {
            // this.Log(LogLevel.Noisy, "Received data 0x{0:X}, current state: {1}  OPI mode? {2}", data, currentOperation.State, opiMode);
            // Eat the first byte when running in OPI mode. The second byte should be the inverse of the first byte.
            if (opiMode)
            {
                if (isFirstOpiByte)
                {
                    isFirstOpiByte = false;
                    firstOpiByte = data;
                    return 0xff;
                }
                else if (currentOperation.State == DecodedOperation.OperationState.RecognizeOperation)
                {
                    if (data != (byte)~firstOpiByte)
                    {
                        this.Log(LogLevel.Error, "Was in OPI mode, but 2nd byte wasn't the inverse of the first byte! (data: {0:X2}  first byte: {1:X2}", data, firstOpiByte);
                    }
                    data = firstOpiByte;
                }
            }

            switch (currentOperation.State)
            {
                case DecodedOperation.OperationState.RecognizeOperation:
                    // When the command is decoded, depending on the operation we will either start accumulating address bytes
                    // or immediately handle the command bytes
                    RecognizeOperation(data);
                    break;
                case DecodedOperation.OperationState.AccumulateCommandAddressBytes:
                    AccumulateAddressBytes(data, DecodedOperation.OperationState.HandleCommand);
                    break;
                case DecodedOperation.OperationState.AccumulateNoDataCommandAddressBytes:
                    AccumulateAddressBytes(data, DecodedOperation.OperationState.HandleNoDataCommand);
                    break;
                case DecodedOperation.OperationState.HandleCommand:
                    // Process the remaining command bytes
                    return HandleCommand(data);
            }

            // Warning: commands without data require immediate handling after the address was accumulated
            if (currentOperation.State == DecodedOperation.OperationState.HandleNoDataCommand)
            {
                HandleNoDataCommand();
            }
            return 0;
        }

        private string InternalBackingFileName;
        private FileStream InternalBackingFile;
        public string BackingFile
        {
            get { return InternalBackingFileName; }
            set
            {
                // If the user passes an empty string, don't do anything.
                if (string.IsNullOrEmpty(value))
                {
                    this.Log(LogLevel.Error, "Unconfiguring backing file");
                    if (InternalBackingFile != null)
                    {
                        InternalBackingFile.Close();
                    }
                    InternalBackingFileName = null;
                    InternalBackingFile = null;
                    return;
                }

                // Ensure the path name is fully-qualified, using the current working
                // directory if necessary.
                if (System.IO.Path.IsPathRooted(value))
                {
                    InternalBackingFileName = value;
                }
                else
                {
                    InternalBackingFileName = System.IO.Path.Combine(Environment.CurrentDirectory, value);
                }
                try
                {
                    this.Log(LogLevel.Debug, "Trying to load or create backing file {0}", InternalBackingFileName);
                    InternalBackingFile = new FileStream(InternalBackingFileName, FileMode.OpenOrCreate, FileAccess.ReadWrite);
                    byte[] copyBuffer = new byte[4096];
                    var currentOffset = 0;

                    // Assume that if the length is 0, then the file was just created and we need
                    // to copy RAM into the newly-created file.
                    if (InternalBackingFile.Length == 0)
                    {
                        this.Log(LogLevel.Debug, "Backing file {0} was created, dumping {1} bytes of ROM to backing file", InternalBackingFileName, underlyingMemory.Size);
                        // 4096 is a safe size, since that is the page size.
                        for (currentOffset = 0; currentOffset < underlyingMemory.Size; currentOffset += copyBuffer.Length)
                        {
                            underlyingMemory.ReadBytes(currentOffset, copyBuffer.Length, copyBuffer, 0);
                            InternalBackingFile.Write(copyBuffer, 0, copyBuffer.Length);
                        }
                    }
                    // If the length is NOT 0, read data from the backing file
                    else
                    {
                        this.Log(LogLevel.Debug, "Backing file {0} was loaded, restoring {1} bytes (vs ROM: {2}) of ROM from file", InternalBackingFileName, InternalBackingFile.Length, underlyingMemory.Size);
                        InternalBackingFile.Seek(0, SeekOrigin.Begin);
                        if (InternalBackingFile.Length != underlyingMemory.Size)
                        {
                            this.Log(LogLevel.Warning, "Backing file {0} was a different size than ROM (file was {1} bytes, ROM is {2} bytes) -- resizing backing file to match ROM size", InternalBackingFileName, InternalBackingFile.Length, underlyingMemory.Size);
                            InternalBackingFile.SetLength(underlyingMemory.Size);
                        }

                        // 4096 is a safe size, since that is the page size.
                        for (currentOffset = 0;
                            (currentOffset < underlyingMemory.Size) && (currentOffset < InternalBackingFile.Length);
                            currentOffset += copyBuffer.Length)
                        {
                            InternalBackingFile.Read(copyBuffer, 0, copyBuffer.Length);
                            underlyingMemory.WriteBytes(currentOffset, copyBuffer);
                        }
                    }
                }
                catch (IOException e)
                {
                    if (InternalBackingFile != null)
                    {
                        InternalBackingFile.Close();
                    }
                    InternalBackingFileName = null;
                    InternalBackingFile = null;
                    throw new RecoverableException(e);
                }
            }
        }


        public MappedMemory UnderlyingMemory => underlyingMemory;

        private void AccumulateAddressBytes(byte addressByte, DecodedOperation.OperationState nextState)
        {
            if (currentOperation.TryAccumulateAddress(addressByte))
            {
                // this.Log(LogLevel.Noisy, "Address accumulated: 0x{0:X}", currentOperation.ExecutionAddress);
                currentOperation.State = nextState;
            }
        }

        private byte[] GetDeviceData()
        {
            var data = new byte[4];
            data[0] = ManufacturerID;
            data[1] = MemoryType;
            data[2] = MemoryDensity;
            data[3] = 0;
            return data;
        }

        private void RecognizeOperation(byte firstByte)
        {
            currentOperation.Operation = DecodedOperation.OperationType.None;
            // currentOperation.AddressLength = 0;
            currentOperation.State = DecodedOperation.OperationState.HandleCommand;
            switch (firstByte)
            {
                case (byte)Commands.ReadID:
                    currentOperation.Operation = DecodedOperation.OperationType.ReadID;
                    if (opiMode)
                    {
                        currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                        currentOperation.AddressLength = 4;
                    }
                    break;
                case (byte)Commands.PageProgram4byte:
                    currentOperation.Operation = DecodedOperation.OperationType.Program;
                    currentOperation.AddressLength = 4;
                    currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                    break;
                case (byte)Commands.WriteEnable:
                    // this.Log(LogLevel.Noisy, "Setting write enable latch");
                    currentOperation.Operation = DecodedOperation.OperationType.None;
                    enable.Value = true;
                    return; //return to prevent further logging
                case (byte)Commands.WriteDisable:
                    // this.Log(LogLevel.Noisy, "Unsetting write enable latch");
                    currentOperation.Operation = DecodedOperation.OperationType.None;
                    enable.Value = false;
                    return; //return to prevent further logging

                case (byte)Commands.SectorErase4byte:
                    currentOperation.Operation = DecodedOperation.OperationType.Erase;
                    currentOperation.EraseSize = DecodedOperation.OperationEraseSize.Sector;
                    currentOperation.AddressLength = 4;
                    currentOperation.State = DecodedOperation.OperationState.AccumulateNoDataCommandAddressBytes;
                    break;

                case (byte)Commands.SubsectorErase4byte4kb:
                    currentOperation.Operation = DecodedOperation.OperationType.Erase;
                    currentOperation.EraseSize = DecodedOperation.OperationEraseSize.Subsector4K;
                    currentOperation.AddressLength = 4;
                    currentOperation.State = DecodedOperation.OperationState.AccumulateNoDataCommandAddressBytes;
                    break;

                case (byte)Commands.ReadStatusRegister:
                    currentOperation.Operation = DecodedOperation.OperationType.ReadRegister;
                    currentOperation.Register = (uint)Register.Status;
                    if (opiMode)
                    {
                        currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                        currentOperation.AddressLength = 4;
                    }
                    break;
                case (byte)Commands.ReadSecurityRegister:
                    currentOperation.Operation = DecodedOperation.OperationType.ReadRegister;
                    currentOperation.Register = (uint)Register.SecurityRegister;
                    if (opiMode)
                    {
                        currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                        currentOperation.AddressLength = 4;
                    }
                    break;
                case (byte)Commands.WriteStatusRegister:
                    currentOperation.Operation = DecodedOperation.OperationType.WriteRegister;
                    currentOperation.Register = (uint)Register.Status;
                    if (opiMode)
                    {
                        currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                        currentOperation.AddressLength = 4;
                    }
                    break;
                case (byte)Commands.WriteSecurityRegister:
                    currentOperation.Operation = DecodedOperation.OperationType.WriteRegister;
                    currentOperation.Register = (uint)Register.SecurityRegister;
                    if (opiMode)
                    {
                        currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                        currentOperation.AddressLength = 4;
                    }
                    break;
                case (byte)Commands.ReadConfigurationRegister2:
                    currentOperation.Operation = DecodedOperation.OperationType.ReadRegister;
                    currentOperation.Register = (uint)Register.ConfigurationRegister2;
                    currentOperation.AddressLength = 4;
                    currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                    break;
                case (byte)Commands.WriteConfigurationRegister2:
                    currentOperation.Operation = DecodedOperation.OperationType.WriteRegister;
                    currentOperation.Register = (uint)Register.ConfigurationRegister2;
                    currentOperation.AddressLength = 4;
                    currentOperation.State = DecodedOperation.OperationState.AccumulateCommandAddressBytes;
                    break;
                case (byte)Commands.ReleaseFromDeepPowerdown:
                    return; //return to prevent further logging
                default:
                    this.Log(LogLevel.Error, "Command decoding failed on byte: 0x{0:X} ({1}).", firstByte, (Commands)firstByte);
                    return;
            }
            // this.Log(LogLevel.Noisy, "Decoded operation: {0}, write enabled {1}", currentOperation, enable.Value);
        }

        private byte HandleCommand(byte data)
        {
            byte result = 0;
            switch (currentOperation.Operation)
            {
                case DecodedOperation.OperationType.ReadFast:
                    // handle dummy byte and switch to read
                    currentOperation.Operation = DecodedOperation.OperationType.Read;
                    currentOperation.CommandBytesHandled--;
                    this.Log(LogLevel.Noisy, "Handling dummy byte in ReadFast operation");
                    break;
                case DecodedOperation.OperationType.Read:
                    result = ReadFromMemory();
                    break;
                case DecodedOperation.OperationType.ReadID:
                    if (currentOperation.CommandBytesHandled < deviceData.Length)
                    {
                        result = deviceData[currentOperation.CommandBytesHandled];
                    }
                    else
                    {
                        this.Log(LogLevel.Error, "Trying to read beyond the length of the device ID table.");
                        result = 0;
                    }
                    break;
                case DecodedOperation.OperationType.Program:
                    if (enable.Value)
                    {
                        WriteToMemory(data);
                        result = data;
                    }
                    else
                    {
                        this.Log(LogLevel.Error, "Memory write operations are disabled.");
                    }
                    break;
                case DecodedOperation.OperationType.ReadRegister:
                    result = ReadRegister((Register)currentOperation.Register);
                    break;
                case DecodedOperation.OperationType.WriteRegister:
                    WriteRegister((Register)currentOperation.Register, data);
                    break;
                case DecodedOperation.OperationType.None:
                    break;
                default:
                    this.Log(LogLevel.Warning, "Unhandled operation encountered while processing command bytes: {0}", currentOperation.Operation);
                    break;
            }
            currentOperation.CommandBytesHandled++;
            // this.Log(LogLevel.Noisy, "Handled command: {0}, returning 0x{1:X}", currentOperation, result);
            return result;
        }

        private void WriteRegister(Register register, byte data)
        {
            if (!enable.Value)
            {
                this.Log(LogLevel.Error, "Trying to write a register, but write enable latch is not set");
                return;
            }
            switch (register)
            {
                case Register.VolatileConfiguration:
                    volatileConfigurationRegister.Write(0, data);
                    break;
                case Register.NonVolatileConfiguration:
                    if ((currentOperation.CommandBytesHandled) >= 2)
                    {
                        this.Log(LogLevel.Error, "Trying to write to register {0} with more than expected 2 bytes.", Register.NonVolatileConfiguration);
                        break;
                    }
                    BitHelper.UpdateWithShifted(ref temporaryNonVolatileConfiguration, data, currentOperation.CommandBytesHandled * 8, 8);
                    if (currentOperation.CommandBytesHandled == 1)
                    {
                        nonVolatileConfigurationRegister.Write(0, (ushort)temporaryNonVolatileConfiguration);
                    }
                    break;
                //listing all cases as other registers are not writable at all
                case Register.EnhancedVolatileConfiguration:
                    enhancedVolatileConfigurationRegister.Write(0, data);
                    break;
                case Register.SecurityRegister:
                    return;
                case Register.ConfigurationRegister2:
                    switch (currentOperation.ExecutionAddress)
                    {
                        case 0x00000000:
                            opiMode = ((data & 3) != 0);
                            if (opiMode)
                            {
                                this.Log(LogLevel.Noisy, "OPI mode enabled");
                            }
                            else
                            {
                                this.Log(LogLevel.Noisy, "OPI mode disabled");
                            }
                            break;
                        case 0x00000200:
                            break;
                        case 0x00000300:
                            break;
                        case 0x00000400:
                            break;
                        case 0x00000500:
                            // CRC chunk size configuration / enabled
                            break;
                        case 0x00000800:
                        case 0x04000800:
                            // ECC fail status
                            break;
                        case 0x00000c00:
                        case 0x04000c00:
                            // ECC fail
                            break;
                        case 0x00000d00:
                        case 0x04000d00:
                            // ECC fail
                            break;
                        case 0x00000e00:
                        case 0x04000e00:
                            // ECC fail
                            break;
                        case 0x00000f00:
                        case 0x04000f00:
                            // ECC fail
                            break;
                        case 0x40000000:
                            // Enable DOPI/SOPI after reset
                            break;
                        case 0x80000000:
                            // CRC error
                            break;
                        default:
                            this.Log(LogLevel.Warning, "Unrecognized CR2 address: {0:X8}", currentOperation.ExecutionAddress);
                            break;
                    }
                    break;
                case Register.Status:
                default:
                    this.Log(LogLevel.Warning, "Trying to write 0x{0} to unsupported register \"{1}\"", data, register);
                    break;
            }
        }

        private byte ReadRegister(Register register)
        {
            switch (register)
            {
                case Register.Status:
                    // The documentation states that at least 1 byte will be read
                    // If more than 1 byte is read, the same byte is returned
                    return statusRegister.Read();
                case Register.FlagStatus:
                    // The documentation states that at least 1 byte will be read
                    // If more than 1 byte is read, the same byte is returned
                    return flagStatusRegister.Read();
                case Register.VolatileConfiguration:
                    // The documentation states that at least 1 byte will be read
                    // If more than 1 byte is read, the same byte is returned
                    return volatileConfigurationRegister.Read();
                case Register.NonVolatileConfiguration:
                    // The documentation states that at least 2 bytes will be read
                    // After all 16 bits of the register have been read, 0 is returned
                    if ((currentOperation.CommandBytesHandled) < 2)
                    {
                        return (byte)BitHelper.GetValue(nonVolatileConfigurationRegister.Read(), currentOperation.CommandBytesHandled * 8, 8);
                    }
                    return 0;
                case Register.SecurityRegister:
                    return 0;
                case Register.EnhancedVolatileConfiguration:
                    return enhancedVolatileConfigurationRegister.Read();
                case Register.ConfigurationRegister2:
                    switch (currentOperation.ExecutionAddress)
                    {
                        case 0x00000000:
                            if (opiMode)
                            {
                                return 2;
                            }
                            return 0;
                        case 0x00000200:
                            return 0;
                        case 0x00000300:
                            return 0;
                        case 0x00000400:
                            return 0;
                        case 0x00000500:
                            // CRC chunk size configuration / enabled
                            return 0;
                        case 0x00000800:
                        case 0x04000800:
                            // ECC fail status
                            return 0;
                        case 0x00000c00:
                        case 0x04000c00:
                            // ECC fail
                            return 0;
                        case 0x00000d00:
                        case 0x04000d00:
                            // ECC fail
                            return 0;
                        case 0x00000e00:
                        case 0x04000e00:
                            // ECC fail
                            return 0;
                        case 0x00000f00:
                        case 0x04000f00:
                            // ECC fail
                            return 0;
                        case 0x40000000:
                            // Enable DOPI/SOPI after reset
                            return 0;
                        case 0x80000000:
                            // CRC error
                            return 0;
                        default:
                            this.Log(LogLevel.Warning, "Unrecognized CR2 address: {0:X8}", currentOperation.ExecutionAddress);
                            return 0;
                    }
                case Register.ExtendedAddress:
                default:
                    this.Log(LogLevel.Warning, "Trying to read from unsupported register \"{0}\"", register);
                    return 0;
            }
        }

        private void HandleNoDataCommand()
        {
            // The documentation describes more commands that don't have any data bytes (just code + address)
            // but at the moment we have implemented just these ones
            switch (currentOperation.Operation)
            {
                case DecodedOperation.OperationType.Erase:
                    if (enable.Value)
                    {
                        if (currentOperation.ExecutionAddress >= underlyingMemory.Size)
                        {
                            this.Log(LogLevel.Error, "Cannot erase memory because current address 0x{0:X} exceeds configured memory size.", currentOperation.ExecutionAddress);
                            return;
                        }
                        switch (currentOperation.EraseSize)
                        {
                            case DecodedOperation.OperationEraseSize.Sector:
                                EraseSector();
                                break;
                            case DecodedOperation.OperationEraseSize.Subsector4K:
                                EraseSubsector4K();
                                break;
                            case DecodedOperation.OperationEraseSize.Die:
                                EraseDie();
                                break;
                            default:
                                this.Log(LogLevel.Warning, "Unsupported erase type: {0}", currentOperation.EraseSize);
                                break;
                        }
                    }
                    else
                    {
                        this.Log(LogLevel.Error, "Erase operations are disabled.");
                    }
                    break;
                default:
                    this.Log(LogLevel.Warning, "Encountered unexpected command: {0}", currentOperation);
                    break;
            }
        }

        private void EraseDie()
        {
            var position = 0;
            var segment = new byte[SegmentSize];
            for (var i = 0; i < SegmentSize; i++)
            {
                segment[i] = EmptySegment;
            }

            while (position < underlyingMemory.Size)
            {
                var length = (int)Math.Min(SegmentSize, underlyingMemory.Size - position);

                underlyingMemory.WriteBytes(position, segment, length);
                if (InternalBackingFile != null)
                {
                    InternalBackingFile.Seek(position, SeekOrigin.Begin);
                    InternalBackingFile.Write(segment, 0, length);
                }
                position += length;
            }
        }

        private void EraseSector()
        {
            var segment = new byte[SegmentSize];
            for (var i = 0; i < SegmentSize; i++)
            {
                segment[i] = EmptySegment;
            }
            // The documentations states that on erase the operation address is
            // aligned to the segment size

            var position = SegmentSize * (currentOperation.ExecutionAddress / SegmentSize);
            underlyingMemory.WriteBytes(position, segment);
            if (InternalBackingFile != null)
            {
                InternalBackingFile.Seek(position, SeekOrigin.Begin);
                InternalBackingFile.Write(segment, 0, segment.Length);
            }
        }

        private void EraseSubsector4K()
        {
            var segment = new byte[4.KB()];
            for (var i = 0; i < 4.KB(); i++)
            {
                segment[i] = EmptySegment;
            }
            // The documentations states that on erase the operation address is
            // aligned to the segment size

            var position = 4.KB() * (currentOperation.ExecutionAddress / 4.KB());
            underlyingMemory.WriteBytes(position, segment);
            if (InternalBackingFile != null)
            {
                InternalBackingFile.Seek(position, SeekOrigin.Begin);
                InternalBackingFile.Write(segment, 0, segment.Length);
            }
        }

        private void WriteToMemory(byte val)
        {
            if (currentOperation.ExecutionAddress + currentOperation.CommandBytesHandled > underlyingMemory.Size)
            {
                this.Log(LogLevel.Error, "Cannot write to address 0x{0:X} because it is bigger than configured memory size.", currentOperation.ExecutionAddress);
                return;
            }

            var position = currentOperation.ExecutionAddress + currentOperation.CommandBytesHandled;
            underlyingMemory.WriteByte(position, val);
            if (InternalBackingFile != null)
            {
                var tmp_byte = new byte[1];
                tmp_byte[0] = val;
                InternalBackingFile.Seek(position, SeekOrigin.Begin);
                InternalBackingFile.Write(tmp_byte, 0, 1);
                InternalBackingFile.Flush();
            }
        }

        private byte ReadFromMemory()
        {
            if (currentOperation.ExecutionAddress + currentOperation.CommandBytesHandled > underlyingMemory.Size)
            {
                this.Log(LogLevel.Error, "Cannot read from address 0x{0:X} because it is bigger than configured memory size.", currentOperation.ExecutionAddress);
                return 0;
            }

            var position = currentOperation.ExecutionAddress + currentOperation.CommandBytesHandled;
            return underlyingMemory.ReadByte(position);
        }

        private DecodedOperation currentOperation;
        private uint temporaryNonVolatileConfiguration; //this should be an ushort, but due to C# type promotions it's easier to use uint

        private readonly byte[] deviceData;
        private readonly int SegmentSize = 64.KB();
        private readonly IFlagRegisterField enable;
        private readonly ByteRegister statusRegister;
        private readonly ByteRegister flagStatusRegister;
        private readonly IFlagRegisterField numberOfAddressBytes;
        private readonly ByteRegister volatileConfigurationRegister;
        private readonly ByteRegister enhancedVolatileConfigurationRegister;
        private readonly WordRegister nonVolatileConfigurationRegister;
        private readonly MappedMemory underlyingMemory;

        private const byte EmptySegment = 0xff;
        private const byte ManufacturerID = 0xC2;
        private const byte RemainingIDBytes = 0x10;
        private const byte MemoryType = 0x80;
        private const byte MemoryDensity = 0x3B;
        private const byte DeviceConfiguration = 0x0;   // standard
        private const byte DeviceGeneration = 0x1;      // 2nd generation
        private const byte ExtendedDeviceID = DeviceGeneration << 6;

        private bool opiMode = false;
        private bool isFirstOpiByte = false;
        private byte firstOpiByte;

        private enum Commands : byte
        {
            // Software RESET Operations
            ResetEnable = 0x66,
            ResetMemory = 0x99,

            // READ ID Operations
            ReadID = 0x9F,
            ReadSerialFlashDiscoveryParameter = 0x5A,

            // READ MEMORY Operations
            Read = 0x03,
            FastRead = 0x0B,

            // READ MEMORY Operations with 4-Byte Address
            Read4byte = 0x13,
            FastRead4byte = 0x0C,

            // WRITE Operations
            WriteEnable = 0x06,
            WriteDisable = 0x04,

            // READ REGISTER Operations
            ReadStatusRegister = 0x05,
            ReadConfigurationRegister2 = 0x71,
            ReadFastBootRegister = 0x16,

            // WRITE REGISTER Operations
            WriteStatusRegister = 0x01,
            WriteConfigurationRegister2 = 0x72,
            WriteFastBootRegister = 0x17,

            // PROGRAM Operations
            PageProgram = 0x02,

            // PROGRAM Operations with 4-Byte Address
            PageProgram4byte = 0x12,

            // ERASE Operations
            SubsectorErase4kb = 0x20,
            SectorErase = 0xD8,
            DieErase = 0x60,
            EraseFastBootRegister = 0x18,

            // ERASE Operations with 4-Byte Address
            SectorErase4byte = 0xDC,
            SubsectorErase4byte4kb = 0x21,
            SubsectorErase4byte32kb = 0x5C,
            DieErase4byte = 0xC7,

            // SUSPEND/RESUME Operations
            ProgramEraseSuspend = 0xB0,
            ProgramEraseResume = 0x30,

            // ONE-TIME PROGRAMMABLE (OTP) Operations
            EnterSecuredOtp = 0xB1,
            ExitSecuredOtp = 0xC1,

            // Deep Power-Down Operations
            EnterDeepPowerDown = 0xB9,
            ReleaseFromDeepPowerdown = 0xAB,

            // ADVANCED SECTOR PROTECTION Operations
            ReadSectorProtection = 0x2D,
            ProgramSectorProtection = 0x2C,
            ReadSpbRegister = 0xE2,
            WriteSpbRegister = 0xE3,
            EraseSpbRegister = 0xE4,
            ReadPassword = 0x27,
            WritePassword = 0x28,
            UnlockPassword = 0x29,

            // ADVANCED SECTOR PROTECTION Operations with 4-Byte Address
            ReadDpbRegister = 0xE0,
            WriteDpbRegister = 0xE1,

            // SECURITY REGISTER
            ReadSecurityRegister = 0x2B,
            WriteSecurityRegister = 0x2F,
        }

        private enum Register : uint
        {
            Status = 1, //starting from 1 to leave 0 as an unused value
            ConfigurationRegister2,
            SecurityRegister,
            // The following are currently unised
            FlagStatus,
            ExtendedAddress,
            NonVolatileConfiguration,
            VolatileConfiguration,
            EnhancedVolatileConfiguration
        }
    }
}
