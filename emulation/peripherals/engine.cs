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
    public class Engine : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Engine(Machine machine)
        {
            this.machine = machine;
            machine.SystemBus.AddWatchpointHook(this.bufferAddress, SysbusAccessWidth.DoubleWord, Access.Write, (cpu, encounteredAddress, width, encounteredValue) =>
            {
                this.Log(LogLevel.Error, "ENGINE: Adding word 0x{0:X} (at address 0x{1:X}) to hash", encounteredValue, encounteredAddress);
                // var inputBuffer = new byte[4];
                // if (this.inputIsSwapped.Value)
                // {
                //     inputBuffer[3] = (byte)(encounteredValue >> 0);
                //     inputBuffer[2] = (byte)(encounteredValue >> 8);
                //     inputBuffer[1] = (byte)(encounteredValue >> 16);
                //     inputBuffer[0] = (byte)(encounteredValue >> 24);
                // }
                // else
                // {
                //     inputBuffer[0] = (byte)(encounteredValue >> 0);
                //     inputBuffer[1] = (byte)(encounteredValue >> 8);
                //     inputBuffer[2] = (byte)(encounteredValue >> 16);
                //     inputBuffer[3] = (byte)(encounteredValue >> 24);
                //     if (this.usingSha256.Value)
                //     {
                //         this.sha256.TransformBlock(inputBuffer, 0, 4, this.hash, 0);
                //     }
                //     else
                //     {
                //         this.sha512.TransformBlock(inputBuffer, 0, 4, this.hash, 0);
                //     }
                // }
                // this.digestedLength += 8;
            });
            RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            Reset();
            DefineRegisters();
        }

        private void DefineRegisters()
        {
            Registers.WINDOW.Define(this)
                .WithValueField(0, 4, name: "WINDOW", valueProviderCallback: (_) => { return this.window; }, changeCallback: (_, value) => { this.window = (byte)(value & 0xf); })
            ;

            Registers.MPSTART.Define(this)
                .WithValueField(0, 10, name: "MPSTART", valueProviderCallback: (_) => { return this.mpstart; }, changeCallback: (_, value) => { this.mpstart = (UInt16)(value & 0x3ff); })
            ;

            Registers.MPLEN.Define(this)
                .WithValueField(0, 10, name: "MPLEN", valueProviderCallback: (_) => { return this.mplen; }, changeCallback: (_, value) => { this.mplen = (UInt16)(value & 0x3ff); })
            ;

            Registers.CONTROL.Define(this)
                .WithFlag(0, name: "GO", changeCallback: (_, value) =>
                {
                    if (value)
                    {
                        this.Log(LogLevel.Error, "Beginning ENGINE execution from 0x{0:X} (program length {1}) using register bank {2}", this.mpstart, this.mplen, this.window);

                    }
                })
                ;

            Registers.MPRESUME.Define(this)
                .WithValueField(0, 10, name: "MPRESUME", valueProviderCallback: (_) => { return this.mpresume; }, changeCallback: (_, value) => { this.mpresume = (UInt16)(value & 0x3ff); })
            ;

            Registers.POWER.Define(this)
                .WithFlag(0, out powerIsOn, FieldMode.Read | FieldMode.Write, name: "ON")
                .WithFlag(1, out pauseRequest, FieldMode.Read | FieldMode.Write, name: "PAUSE_REQ")
            ;

            Registers.STATUS.Define(this)
                .WithFlag(0, name: "RUNNING", valueProviderCallback: (_) => false)
                .WithValueField(1, 10, name: "MPC", valueProviderCallback: (_) => 0)
                .WithFlag(11, name: "PAUSE_GNT", valueProviderCallback: (_) => false)
            ;

            Registers.EV_STATUS.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "FINISHED", valueProviderCallback: _ => illegalOpcodeStatus)
                .WithFlag(1, FieldMode.Read, name: "ILLEGAL_OPCODE", valueProviderCallback: _ => finishedStatus)
            ;

            Registers.EV_PENDING.Define32(this)
                .WithFlag(0, out illegalOpcodePending, FieldMode.Read | FieldMode.WriteOneToClear, name: "FINISHED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out finishedPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "ILLEGAL_OPCODE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EV_ENABLE.Define32(this)
                .WithFlag(0, out illegalOpcodeEnabled, name: "FINISHED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out finishedEnabled, name: "ILLEGAL_OPCODE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.INSTRUCTION.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { return 11 /* Opcode FIN */; })
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.illegalOpcodeStatus && this.illegalOpcodeEnabled.Value)
            {
                this.illegalOpcodePending.Value = true;
            }
            if (this.finishedStatus && this.finishedEnabled.Value)
            {
                this.finishedPending.Value = true;
            }
            IRQ.Set((this.illegalOpcodePending.Value && this.illegalOpcodeEnabled.Value)
            || (this.finishedPending.Value && this.finishedEnabled.Value));
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
            this.window = 0;
            this.mpstart = 0;
            this.mplen = 0;
            this.mpresume = 0;
            this.mpc = 0;
            if (this.powerIsOn != null) {
                this.powerIsOn.Value = false;
            }
        }

        private readonly Machine machine;

        public GPIO IRQ { get; private set; }

        private byte window;
        private UInt16 mpstart;
        private UInt16 mplen;
        private UInt16 mpresume;
        private UInt16 mpc;
        private IFlagRegisterField powerIsOn;
        private IFlagRegisterField pauseRequest;
        private readonly uint bufferAddress = 0xE0020000;

        private IFlagRegisterField illegalOpcodeEnabled;
        private IFlagRegisterField illegalOpcodePending;
        private bool illegalOpcodeStatus;

        private IFlagRegisterField finishedEnabled;
        private IFlagRegisterField finishedPending;
        private bool finishedStatus;

        private enum Registers
        {
            WINDOW = 0x00,
            MPSTART = 0x04,
            MPLEN = 0x08,
            CONTROL = 0x0c,
            MPRESUME = 0x10,
            POWER = 0x14,
            STATUS = 0x18,
            EV_STATUS = 0x1c,
            EV_PENDING = 0x20,
            EV_ENABLE = 0x24,
            INSTRUCTION = 0x28
        }
    }
}
