//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Threading;
using System.Text;
using System.Linq;
using System.Globalization;
using System.Numerics;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Utilities.Binding;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{

    public class BetrustedJtag : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public BetrustedJtag(Machine machine)
        {
            this.RegistersCollection = new DoubleWordRegisterCollection(this);
            DefineRegisters();
            Reset();
        }

        private void DefineRegisters()
        {
            Registers.Next.Define32(this)
                .WithValueField(0, 2, FieldMode.Write, writeCallback: (_, val) =>
                {
                    var tdi = ((val >> 0) & 1) == 1;
                    var tms = ((val >> 1) & 1) == 1;
                    var old_state = State;
                    var is_new_state = AdvanceStateMachine(tdi, tms);
                    // this.Log(LogLevel.Error, "JTAG packet -- TDI: {0},  TMS: {1}, State: {2} (will be: {3}; is new? {4})", tdi, tms, old_state, State, is_new_state);

                    switch (old_state)
                    {
                        case JtagState.SelectDrScan:
                            next_tdo_bit = false;
                            break;

                        case JtagState.CaptureDr:
                            count = 0;
                            data_register = 0;
                            next_tdo_bit = false;
                            break;

                        case JtagState.ShiftDr:
                            if (tdi)
                            {
                                data_register |= (1UL << (byte)count);
                            }
                            next_tdo_bit = (output_value & (1UL << (byte)count)) != 0;
                            count += 1;
                            break;

                        case JtagState.Exit1Dr:
                            this.Log(LogLevel.Info, "JTAG data register value: {0:X08}", data_register);
                            next_tdo_bit = false;
                            break;

                        case JtagState.CaptureIr:
                            count = 0;
                            instruction_register = 0;
                            next_tdo_bit = false;
                            break;

                        case JtagState.ShiftIr:
                            if (tdi)
                            {
                                instruction_register |= (1UL << (byte)count);
                            }
                            next_tdo_bit = (output_value & (1UL << (byte)count)) != 0;
                            count += 1;
                            break;

                        case JtagState.Exit1Ir:
                            this.Log(LogLevel.Info, "JTAG instruction register value: {0:X08}", instruction_register);
                            next_tdo_bit = false;
                            break;

                        case JtagState.UpdateIr:
                            if (instruction_register == 9)
                            {
                                output_value = IDCODE;
                            }
                            break;

                        default:
                            break;
                    }
                })
            ;

            Registers.Tdo.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "TDO", valueProviderCallback: _ =>
                {
                    // this.Log(LogLevel.Info, "Shifting out value {0:X08} bit {1}: {2}", output_value, count, (output_value & (1UL << (byte)count)) != 0);
                    return next_tdo_bit;
                })
                .WithFlag(1, FieldMode.Read, name: "READY", valueProviderCallback: _ =>
                {
                    // Indicate we're always ready for another bit
                    return true;
                })
            ;
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        // Advance the JTAG state machine. Returns `true` if a new state is entered.
        private bool AdvanceStateMachine(bool tdi, bool tms)
        {
            switch (State)
            {
                case JtagState.TestLogicReset:
                    if (tms)
                    {
                        return false;
                    }
                    else
                    {
                        State = JtagState.RunTestIdle;
                    }
                    break;

                case JtagState.RunTestIdle:
                    if (tms)
                    {
                        State = JtagState.SelectDrScan;
                    }
                    else
                    {
                        return false;
                    }
                    break;

                case JtagState.SelectDrScan:
                    if (tms)
                    {
                        State = JtagState.SelectIrScan;
                    }
                    else
                    {
                        State = JtagState.CaptureDr;
                    }
                    break;
                case JtagState.CaptureDr:
                    if (tms)
                    {
                        State = JtagState.Exit1Dr;
                    }
                    else
                    {
                        State = JtagState.ShiftDr;
                    }
                    break;
                case JtagState.ShiftDr:
                    if (tms)
                    {
                        State = JtagState.Exit1Dr;
                    }
                    else
                    {
                        return false;
                    }
                    break;
                case JtagState.Exit1Dr:
                    if (tms)
                    {
                        State = JtagState.UpdateDr;
                    }
                    else
                    {
                        State = JtagState.PauseDr;
                    }
                    break;
                case JtagState.PauseDr:
                    if (tms)
                    {
                        State = JtagState.Exit2Dr;
                    }
                    else
                    {
                        return false;
                    }
                    break;
                case JtagState.Exit2Dr:
                    if (tms)
                    {
                        State = JtagState.UpdateDr;
                    }
                    else
                    {
                        State = JtagState.ShiftDr;
                    }
                    break;
                case JtagState.UpdateDr:
                    if (tms)
                    {
                        State = JtagState.SelectDrScan;
                    }
                    else
                    {
                        State = JtagState.RunTestIdle;
                    }
                    break;

                case JtagState.SelectIrScan:
                    if (tms)
                    {
                        State = JtagState.TestLogicReset;
                    }
                    else
                    {
                        State = JtagState.CaptureIr;
                    }
                    break;
                case JtagState.CaptureIr:
                    if (tms)
                    {
                        State = JtagState.Exit1Ir;
                    }
                    else
                    {
                        State = JtagState.ShiftIr;
                    }
                    break;
                case JtagState.ShiftIr:
                    if (tms)
                    {
                        State = JtagState.Exit1Ir;
                    }
                    else
                    {
                        return false;
                    }
                    break;
                case JtagState.Exit1Ir:
                    if (tms)
                    {
                        State = JtagState.UpdateIr;
                    }
                    else
                    {
                        State = JtagState.PauseIr;
                    }
                    break;
                case JtagState.PauseIr:
                    if (tms)
                    {
                        State = JtagState.Exit2Ir;
                    }
                    else
                    {
                        return false;
                    }
                    break;
                case JtagState.Exit2Ir:
                    if (tms)
                    {
                        State = JtagState.UpdateIr;
                    }
                    else
                    {
                        State = JtagState.ShiftIr;
                    }
                    break;
                case JtagState.UpdateIr:
                    if (tms)
                    {
                        State = JtagState.SelectDrScan;
                    }
                    else
                    {
                        State = JtagState.RunTestIdle;
                    }
                    break;
            }
            return true;
        }

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        private ulong data_register;
        private ulong instruction_register;
        private ulong output_value;
        private ulong value;
        private ulong count;
        public ulong IDCODE = 0x362f093;
        bool next_tdo_bit;

        public void Reset()
        {
            State = JtagState.TestLogicReset;
            RegistersCollection.Reset();
        }

        private enum JtagState
        {
            TestLogicReset,
            RunTestIdle,
            SelectDrScan,
            CaptureDr,
            ShiftDr,
            Exit1Dr,
            PauseDr,
            Exit2Dr,
            UpdateDr,
            SelectIrScan,
            CaptureIr,
            ShiftIr,
            Exit1Ir,
            PauseIr,
            Exit2Ir,
            UpdateIr,
        }

        private JtagState State;

        private enum Registers
        {
            Next = 0x00,
            Tdo = 0x04
        }
    }
}
