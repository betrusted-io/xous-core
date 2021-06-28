//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Collections.Generic;
using System.Security.Cryptography;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class Sha512 : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Sha512(Machine machine)
        {
            this.machine = machine;
            machine.SystemBus.AddWatchpointHook(this.bufferAddress, SysbusAccessWidth.DoubleWord, Access.Write, (cpu, encounteredAddress, width, encounteredValue) =>
            {
                this.Log(LogLevel.Error, "Adding word 0x{0:X} (at address 0x{1:X}) to hash", encounteredValue, encounteredAddress);
                var inputBuffer = new byte[4];
                if (this.inputIsSwapped.Value)
                {
                    inputBuffer[3] = (byte)(encounteredValue >> 0);
                    inputBuffer[2] = (byte)(encounteredValue >> 8);
                    inputBuffer[1] = (byte)(encounteredValue >> 16);
                    inputBuffer[0] = (byte)(encounteredValue >> 24);
                }
                else
                {
                    inputBuffer[0] = (byte)(encounteredValue >> 0);
                    inputBuffer[1] = (byte)(encounteredValue >> 8);
                    inputBuffer[2] = (byte)(encounteredValue >> 16);
                    inputBuffer[3] = (byte)(encounteredValue >> 24);
                    if (this.usingSha256.Value)
                    {
                        this.sha256.TransformBlock(inputBuffer, 0, 4, this.hash, 0);
                    }
                    else
                    {
                        this.sha512.TransformBlock(inputBuffer, 0, 4, this.hash, 0);
                    }
                }
                this.digestedLength += 8;
            });
            RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            this.sha512 = new SHA512Managed();
            this.sha256 = new SHA256Managed();
            this.hash = new byte[64];
            Reset();
            DefineRegisters();
        }

        private uint ResultAtOffset(int offset)
        {
            if (usingSha256.Value && offset >= 32)
            {
                offset -= 32;
                if (outputIsSwapped.Value)
                {
                    return ((uint)this.sha256.Hash[offset + 0] << 0)
                    | ((uint)this.sha256.Hash[offset + 1] << 8)
                    | ((uint)this.sha256.Hash[offset + 2] << 16)
                    | ((uint)this.sha256.Hash[offset + 3] << 24);
                }
                else
                {
                    return ((uint)this.sha256.Hash[offset + 3] << 0)
                    | ((uint)this.sha256.Hash[offset + 2] << 8)
                    | ((uint)this.sha256.Hash[offset + 1] << 16)
                    | ((uint)this.sha256.Hash[offset + 0] << 24);
                }
            }

            // Using sha 512
            if (outputIsSwapped.Value)
            {
                return ((uint)this.sha512.Hash[offset + 0] << 0)
                | ((uint)this.sha512.Hash[offset + 1] << 8)
                | ((uint)this.sha512.Hash[offset + 2] << 16)
                | ((uint)this.sha512.Hash[offset + 3] << 24);
            }
            else
            {
                return ((uint)this.sha512.Hash[offset + 3] << 0)
                | ((uint)this.sha512.Hash[offset + 2] << 8)
                | ((uint)this.sha512.Hash[offset + 1] << 16)
                | ((uint)this.sha512.Hash[offset + 0] << 24);
            }
        }

        private void DefineRegisters()
        {
            Registers.POWER.Define(this)
                .WithFlag(0, out powerIsOn, FieldMode.Read | FieldMode.Write, name: "ON")
            ;

            Registers.CONFIG.Define(this)
                .WithFlag(0, out shaIsEnabled, FieldMode.Read | FieldMode.Write, name: "SHA_EN")
                .WithFlag(1, out inputIsSwapped, FieldMode.Read | FieldMode.Write, name: "ENDIAN_SWAP")
                .WithFlag(2, out outputIsSwapped, FieldMode.Read | FieldMode.Write, name: "DIGEST_SWAP")
                .WithFlag(3, out usingSha256, FieldMode.Read | FieldMode.Write, name: "DIGEST_SWAP")
            ;

            Registers.COMMAND.Define(this)
                .WithFlag(0, name: "HASH_START", changeCallback: (_, value) =>
                {
                    if (value)
                    {
                        if (usingSha256.Value)
                        {
                            sha256.Initialize();
                        }
                        else
                        {
                            sha512.Initialize();
                        }
                        this.digestedLength = 0;
                    }
                })
                .WithFlag(1, name: "HASH_PROCESS", changeCallback: (_, value) =>
                {
                    if (value)
                    {
                        var dummy = new byte[0];
                        if (usingSha256.Value)
                        {
                            this.sha256.TransformFinalBlock(dummy, 0, 0);
                            this.sha256.Hash.CopyTo(this.hash, 0);
                        }
                        else
                        {
                            this.sha512.TransformFinalBlock(dummy, 0, 0);
                            this.sha512.Hash.CopyTo(this.hash, 0);
                        }
                    }
                })
            ;

            Registers.DIGEST01.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(4); })
            ;
            Registers.DIGEST00.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(0); })
            ;
            Registers.DIGEST11.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(12); })
            ;
            Registers.DIGEST10.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(8); })
            ;
            Registers.DIGEST21.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(20); })
            ;
            Registers.DIGEST20.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(16); })
            ;
            Registers.DIGEST31.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(28); })
            ;
            Registers.DIGEST30.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(24); })
            ;
            Registers.DIGEST41.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(36); })
            ;
            Registers.DIGEST40.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(32); })
            ;
            Registers.DIGEST51.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(44); })
            ;
            Registers.DIGEST50.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(40); })
            ;
            Registers.DIGEST61.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(52); })
            ;
            Registers.DIGEST60.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(48); })
            ;
            Registers.DIGEST71.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(60); })
            ;
            Registers.DIGEST70.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return ResultAtOffset(56); })
            ;
            Registers.MSG_LENGTH1.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return (uint)(this.digestedLength >> 32); })
            ;
            Registers.MSG_LENGTH0.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return (uint)this.digestedLength; })
            ;

            Registers.EV_STATUS.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "ERR_VALID", valueProviderCallback: _ => errValidStatus)
                .WithFlag(1, FieldMode.Read, name: "FIFO_FULL", valueProviderCallback: _ => fifoFullStatus)
                .WithFlag(2, FieldMode.Read, name: "SHA512_DONE", valueProviderCallback: _ => sha512DoneStatus)
            ;

            Registers.EV_PENDING.Define32(this)
                .WithFlag(0, out errValidPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "ERR_VALID", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out fifoFullPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "FIFO_FULL", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(2, out sha512DonePending, FieldMode.Read | FieldMode.WriteOneToClear, name: "SHA512_DONE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EV_ENABLE.Define32(this)
                .WithFlag(0, out errValidEnabled, name: "ERR_VALID", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out fifoFullEnabled, name: "FIFO_FULL", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(2, out sha512DoneEnabled, name: "SHA512_DONE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.FIFO.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return 0; })
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.errValidStatus && this.errValidEnabled.Value)
            {
                this.errValidPending.Value = true;
            }
            if (this.fifoFullStatus && this.fifoFullEnabled.Value)
            {
                this.fifoFullPending.Value = true;
            }
            if (this.sha512DoneStatus && this.sha512DoneEnabled.Value)
            {
                this.sha512DonePending.Value = true;
            }
            IRQ.Set((this.errValidPending.Value && this.errValidEnabled.Value)
            || (this.fifoFullPending.Value && this.fifoFullEnabled.Value)
            || (this.sha512DonePending.Value && this.sha512DoneEnabled.Value));
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Reset()
        {
            this.sha256.Initialize();
            this.sha512.Initialize();
            this.digestedLength = 0;
        }

        private readonly Machine machine;

        private SHA512Managed sha512;
        private SHA256Managed sha256;
        private byte[] hash;
        public GPIO IRQ { get; private set; }

        private IFlagRegisterField powerIsOn;
        private IFlagRegisterField shaIsEnabled;
        private IFlagRegisterField inputIsSwapped;
        private IFlagRegisterField outputIsSwapped;
        private IFlagRegisterField usingSha256;

        private IFlagRegisterField errValidEnabled;
        private IFlagRegisterField errValidPending;
        private bool errValidStatus;

        private IFlagRegisterField fifoFullEnabled;
        private IFlagRegisterField fifoFullPending;
        private bool fifoFullStatus;

        private IFlagRegisterField sha512DoneEnabled;
        private IFlagRegisterField sha512DonePending;
        private bool sha512DoneStatus;

        private readonly uint bufferAddress = 0xE0002000;
        private UInt64 digestedLength;
        private enum Registers
        {
            POWER = 0x00,
            CONFIG = 0x04,
            COMMAND = 0x08,
            DIGEST01 = 0x0c,
            DIGEST00 = 0x10,
            DIGEST11 = 0x14,
            DIGEST10 = 0x18,
            DIGEST21 = 0x1c,
            DIGEST20 = 0x20,
            DIGEST31 = 0x24,
            DIGEST30 = 0x28,
            DIGEST41 = 0x2c,
            DIGEST40 = 0x30,
            DIGEST51 = 0x34,
            DIGEST50 = 0x38,
            DIGEST61 = 0x3c,
            DIGEST60 = 0x40,
            DIGEST71 = 0x44,
            DIGEST70 = 0x48,
            MSG_LENGTH1 = 0x4c,
            MSG_LENGTH0 = 0x50,
            EV_STATUS = 0x54,
            EV_PENDING = 0x58,
            EV_ENABLE = 0x5c,
            FIFO = 0x60
        }
    }
}
