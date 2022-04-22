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

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class EngineRam : IDoubleWordPeripheral, IKnownSize
    {
        public EngineRam(Machine machine, Engine engine, long size)
        {
            this.machine = machine;
            this.engine = engine;
            this.size = size;
            this.data = new UInt32[size / 4];
            this.FIELD_MAX = BigInteger.Pow(2, 255) - 19;
            this.UINT_MAX = BigInteger.Pow(2, 256) - 1;
            this.ZERO = new BigInteger(0);
            this.ONE = new BigInteger(1);

            this.AM24 = new BigInteger(121665); // (A-2)/4
            this.AP24 = new BigInteger(121666); // (A+2)/4
            this.FIVE = new BigInteger(5);
            this.TEN = new BigInteger(10);
            this.TWENTY = new BigInteger(20);
            this.FIFTY = new BigInteger(50);
            this.ONE_HUNDRED = new BigInteger(100);
            this.registers = new BigInteger[32];
            for (var i = 0; i < this.registers.Length; i++)
            {
                this.registers[i] = this.ZERO;
            }
        }

        public void SyncRegistersFromRam(ulong rf)
        {
            for (var i = 0; i < this.registers.Length; i++)
            {
                this.registers[i] = ReadRegisterFromRam(rf, (ulong)i);
            }
        }

        public void SyncRegistersToRam(ulong rf)
        {
            for (var i = 0; i < this.registers.Length; i++)
            {
                WriteRegisterToRam(rf, (ulong)i, this.registers[i]);
            }
        }

        public BigInteger UINT_MAX;
        public BigInteger ZERO;
        public BigInteger ONE;
        public BigInteger AM24;
        public BigInteger FIELD_MAX;
        public BigInteger AP24;
        public BigInteger FIVE;
        public BigInteger TEN;
        public BigInteger TWENTY;
        public BigInteger FIFTY;
        public BigInteger ONE_HUNDRED;

        private Machine machine;
        private Engine engine;
        private long size;
        public UInt32[] data;
        public BigInteger[] registers;
        public long Size { get { return size; } }

        public void WriteDoubleWord(long address, uint value)
        {
            this.data[address / 4] = value;
        }

        public uint ReadDoubleWord(long offset)
        {
            return this.data[offset / 4];
        }

        public BigInteger ReadRegister(ulong register, bool constant)
        {
            // this.Log(LogLevel.Error, "Reading register {0} (const? {1}) from rf {2}", register, constant, rf);
            if (constant)
            {
                switch (register)
                {
                    case 0: return this.ZERO;
                    case 1: return this.ONE;
                    case 2: return this.AM24;
                    case 3: return this.FIELD_MAX;
                    case 4: return this.AP24;
                    case 5: return this.FIVE;
                    case 6: return this.TEN;
                    case 7: return this.TWENTY;
                    case 8: return this.FIFTY;
                    case 9: return this.ONE_HUNDRED;
                    default: return this.ZERO;
                }
            }
            return this.registers[register];
        }
        public BigInteger ReadRegisterFromRam(ulong rf, ulong register)
        {
            var src_bytes = new byte[33]; // Keep leading byte 0 to force unsigned

            // if ((rf >= 16) || (register >= 32))
            // {
            //     throw new Exception(String.Format("Register #{0} or RegisterFile {1} is out of range", register, rf));
            // }

            for (ulong i = 0; i < 32; i += 4)
            {
                var offset = 0x10000 + (rf * 1024) + (register * 32) + i;
                var bytes = BitConverter.GetBytes(this.data[offset / 4]);
                if (bytes.Length != 4)
                {
                    throw new Exception("Bytes was not equal to 4");
                }
                for (ulong j = 0; j < (ulong)bytes.Length; j++)
                {
                    if ((i + j) >= 32)
                    {
                        throw new Exception("i+j>=32");
                    }
                    src_bytes[i + j] = bytes[j];
                }
            }
            // this.Log(LogLevel.Error, "Returning read value from {0}-byte array", src_bytes.Length);
            return new BigInteger(src_bytes);
        }

        public void WriteRegisterToRam(ulong rf, ulong register, BigInteger value)
        {
            // if ((rf >= 16) || (register >= 32))
            // {
            //     throw new Exception(String.Format("Register #{0} or RegisterFile {1} is out of range", register, rf));
            // }

            var bytes = value.ToByteArray();
            if (bytes.Length < 32)
            {
                bytes = bytes.Concat(new byte[32 - bytes.Length]);
            }

            var offset = 0x10000 + (rf * 1024) + (register * 32);
            for (uint i = 0; i < 32; i += 4)
            {
                this.data[(offset + i) / 4] = (((uint)bytes[i + 0]) << 0)
                                            | (((uint)bytes[i + 1]) << 8)
                                            | (((uint)bytes[i + 2]) << 16)
                                            | ((uint)(bytes[i + 3]) << 24);
            }
        }

        public void WriteRegister(ulong register, BigInteger value)
        {
            this.registers[register] = value & this.UINT_MAX;
        }

        public String FormatValue(BigInteger value)
        {
            var trimmed_value = value & this.UINT_MAX;
            var bytes = trimmed_value.ToByteArray();
            if (bytes.Length < 32)
            {
                bytes = bytes.Concat(new byte[32 - bytes.Length]);
            }
            StringBuilder hex = new StringBuilder(bytes.Length * 2);
            foreach (byte b in bytes.Reverse())
                hex.AppendFormat("{0:x2}", b);
            return hex.ToString();
        }

        public void Reset()
        {
            for (var i = 0; i < this.data.Length; i++)
            {
                this.data[i] = 0;
            }
        }
    }

    public class Opcode : IFormattable
    {
        public Opcode(UInt32 op)
        {
            this.Op = (op >> 0) & 0x3f;

            this.Ra = (op >> 6) & 0x1f;
            this.Ca = ((op >> 11) & 1) == 1;

            this.Rb = (op >> 12) & 0x1f;
            this.Cb = ((op >> 17) & 1) == 1;

            this.Wd = (op >> 18) & 0x1f;

            this.Immediate = (int)(op >> 23);
            if ((this.Immediate & (1 << 8)) != 0)
            {
                this.Immediate = this.Immediate - (1 << 9);
            }
        }

        public String ConstantName(uint idx)
        {
            switch (idx)
            {
                case 0: return "zero";
                case 1: return "one";
                case 2: return "am24";
                case 3: return "field";
                case 4: return "ap24";
                case 5: return "five";
                case 6: return "ten";
                case 7: return "twenty";
                case 8: return "fifty";
                case 9: return "one_hundred";
                default: return "undef";
            }
        }

        public String RaName()
        {
            if (this.Ca)
            {
                return "#" + this.ConstantName(this.Ra).ToUpper();
            }
            return "r" + this.Ra;
        }

        public String RbName()
        {
            if (this.Cb)
            {
                return "#" + this.ConstantName(this.Rb).ToUpper();
            }
            return "r" + this.Rb;
        }

        public String WdName()
        {
            return "r" + this.Wd;
        }

        public string Mnemonic()
        {
            switch (this.Op)
            {
                case 0: return "PSA";
                case 1: return "PSB";
                case 2: return "MSK";
                case 3: return "XOR";
                case 4: return "NOT";
                case 5: return "ADD";
                case 6: return "SUB";
                case 7: return "MUL";
                case 8: return "TRD";
                case 9: return "BRZ";
                case 10: return "FIN";
                case 11: return "SHL";
                case 12: return "XBT";
                default: return "UDF";
            }
        }

        public string ToString(string format, IFormatProvider provider)
        {
            if ((this.Op == 0) || (this.Op == 4) || (this.Op == 8) || (this.Op == 11) || (this.Op == 12))
            {
                return String.Format("{0} {1}, {2}", this.Mnemonic(), this.WdName(), this.RaName());
            }
            else if (this.Op == 1)
            {
                return String.Format("{0} {1}, {2}", this.Mnemonic(), this.WdName(), this.RbName());
            }
            else if (this.Op == 9)
            {
                return String.Format("{0} pc + {1}, {2}", this.Mnemonic(), this.Immediate, this.RaName());
            }
            else if (this.Op == 10)
            {
                return this.Mnemonic();
            }
            else if ((this.Op == 2) || (this.Op == 3) || (this.Op == 5) || (this.Op == 6) || (this.Op == 7)) // OK
            {
                return String.Format("{0} {1}, {2}, {3}", this.Mnemonic(), this.WdName(), this.RaName(), this.RbName());
            }
            else
            {
                return "undef";
            }
        }

        public UInt32 Op;
        public UInt32 Ra;
        public bool Ca;
        public UInt32 Rb;
        public bool Cb;
        public UInt32 Wd;
        public Int32 Immediate;
    }


    public class Engine : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Engine(Machine machine, uint memAddr, uint memSize)
        {
            this.machine = machine;
            this.engineRam = new EngineRam(machine, this, memSize);
            machine.SystemBus.Register(this.engineRam, new BusRangeRegistration(memAddr, memSize));

            this.RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            Reset();
            DefineRegisters();
        }

        ~Engine()
        {
            if ((this.engineExecution != null) && (this.engineExecution.IsAlive))
            {
                this.engineExecution.Abort();
            }
        }

        private void EngineThread()
        {
            // this.Log(LogLevel.Error, "ENGINE: Beginning execution. Current MPC: 0x{0:X}", this.mpstart);
            try
            {
                // Interlocked.Exchange(ref this.mpc, this.mpstart);
                this.mpc = this.mpstart;
                var window = this.window;
                this.engineRam.SyncRegistersFromRam(window);

                while (true)
                {
                    if (this.mpc >= this.mplen)
                    {
                        throw new Exception(String.Format("mpc exceeded program length of {0}", this.mplen));
                    }
                    var op = new Opcode(this.engineRam.data[this.mpc]);

                    // this.Log(LogLevel.Error, "RAM[0x{0:X}] {1}", this.mpc, op);

                    switch (op.Op)
                    {
                        case 0: // PSA
                            // this.engineRam.WriteRegister(op.Wd, this.engineRam.ReadRegister(op.Ra, op.Ca));
                            this.engineRam.WriteRegister(op.Wd, this.engineRam.ReadRegister(op.Ra, op.Ca));
                            break;

                        case 1: // PSB
                            this.engineRam.WriteRegister(op.Wd, this.engineRam.ReadRegister(op.Rb, op.Cb));
                            break;

                        case 2: // MSK
                            if ((this.engineRam.ReadRegister(op.Ra, op.Ca) & 1) != 0)
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.ReadRegister(op.Rb, op.Cb));
                            }
                            else
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.ZERO);
                            }
                            break;

                        case 3: // XOR
                            this.engineRam.WriteRegister(op.Wd,
                                (BigInteger.Pow(2, 256) | this.engineRam.ReadRegister(op.Ra, op.Ca)) ^ this.engineRam.ReadRegister(op.Rb, op.Cb));
                            break;

                        case 4: // NOT
                            this.engineRam.WriteRegister(op.Wd,
                                ~(this.engineRam.ReadRegister(op.Ra, op.Ca) | BigInteger.Pow(2, 256)));
                            break;

                        case 5: // ADD
                            this.engineRam.WriteRegister(op.Wd,
                                this.engineRam.ReadRegister(op.Ra, op.Ca) + this.engineRam.ReadRegister(op.Rb, op.Cb));
                            break;

                        case 6: // SUB
                            {
                                var left = this.engineRam.ReadRegister(op.Ra, op.Ca);
                                var right = this.engineRam.ReadRegister(op.Rb, op.Cb);
                                var result = left - right;
                                // Wrap integer
                                if (result < 0)
                                {
                                    result = BigInteger.Pow(2, 256) - result;
                                }
                                this.engineRam.WriteRegister(op.Wd, result);
                            }
                            break;

                        case 7: // MUL
                            this.engineRam.WriteRegister(op.Wd,
                                (this.engineRam.ReadRegister(op.Ra, op.Ca)
                                * this.engineRam.ReadRegister(op.Rb, op.Cb))
                                    % this.engineRam.FIELD_MAX
                                );
                            break;

                        case 8: // TRD
                            if (this.engineRam.ReadRegister(op.Ra, op.Ca) >= this.engineRam.FIELD_MAX)
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.FIELD_MAX);
                            }
                            else
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.ZERO);
                            }
                            break;

                        case 9: // BRZ
                            if (this.engineRam.ReadRegister(op.Ra, op.Ca) == 0)
                            {
                                this.mpc = (ushort)((int)this.mpc + op.Immediate);
                            }
                            break;

                        case 10: // FIN
                            break;

                        case 11: // SHL
                            this.engineRam.WriteRegister(op.Wd, this.engineRam.ReadRegister(op.Ra, op.Ca) << 1);
                            break;

                        case 12: // XBT
                            if ((this.engineRam.ReadRegister(op.Ra, op.Ca) & (this.engineRam.ONE << 254)) != 0)
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.ONE);
                            }
                            else
                            {
                                this.engineRam.WriteRegister(op.Wd, this.engineRam.ZERO);
                            }
                            break;

                        default:
                            this.illegalOpcodeStatus = true;
                            throw new Exception("Unhandled opcode");
                    }
                    //this.Log(LogLevel.Error, "RAM[0x{0:X}] {1}\n   Ra:{2:x}\n   Rb:{3:x}\n   W:{4:x}", this.mpc, op,
                    //    this.engineRam.ReadRegister(op.Ra, op.Ca),
                    //    this.engineRam.ReadRegister(op.Rb, op.Cb),
                    //    this.engineRam.ReadRegister(op.Wd, false)
                    //);

                    // Interlocked.Increment(ref this.mpc);
                    this.mpc += 1;
                    if (op.Op == 10)
                    {
                        break;
                    }
                    // this.Log(LogLevel.Error, "    (Result) {0}", this.engineRam.FormatValue(this.engineRam.ReadRegister(op.Wd, false)));
                }
            }
            catch (Exception e)
            {
                this.Log(LogLevel.Error, "ENGINE execution exception {0}", e);
            }
            this.finishedStatus = true;
            // this.engineExecution = null;
            this.engineRam.SyncRegistersToRam(window);
            this.UpdateInterrupts();
            this.finishedStatus = false;
            this.illegalOpcodeStatus = false;
            // this.Log(LogLevel.Error, "ENGINE finished execution");
        }

        private void DefineRegisters()
        {
            Registers.WINDOW.Define(this)
                .WithValueField(0, 4, name: "WINDOW", valueProviderCallback: (_) => this.window, writeCallback: (_, value) => this.window = (byte)(value & 0xf))
            ;

            Registers.MPSTART.Define(this)
                .WithValueField(0, 11, name: "MPSTART", valueProviderCallback: (_) => this.mpstart, writeCallback: (_, value) => this.mpstart = (UInt16)(value & 0x7ff))
            ;

            Registers.MPLEN.Define(this)
                .WithValueField(0, 11, name: "MPLEN", valueProviderCallback: _ => this.mplen, writeCallback: (_, value) => this.mplen = (UInt16)(value & 0x7ff))
            ;

            Registers.CONTROL.Define(this)
                .WithFlag(0, FieldMode.Write, name: "GO", writeCallback: (_, value) =>
                {
                    if (value)
                    {
                        if ((this.engineExecution != null) && (this.engineExecution.IsAlive))
                        {
                            this.Log(LogLevel.Error, "ENGINE already executing -- not re-running");
                        }
                        else
                        {
                            this.engineExecution = new Thread(this.EngineThread) { Name = "Engine Execution", IsBackground = true };
                            this.engineExecution.Start();
                        }
                    }
                })
                ;

            Registers.MPRESUME.Define(this)
                .WithValueField(0, 11, name: "MPRESUME", valueProviderCallback: _ => this.mpresume, writeCallback: (_, value) => { this.mpresume = (UInt16)(value & 0x7ff); })
            ;

            Registers.POWER.Define(this)
                .WithFlag(0, out powerIsOn, FieldMode.Read | FieldMode.Write, name: "ON")
                .WithFlag(1, out pauseRequest, FieldMode.Read | FieldMode.Write, name: "PAUSE_REQ")
            ;

            Registers.STATUS.Define(this)
                .WithFlag(0, name: "RUNNING", valueProviderCallback: (_) => this.engineExecution.IsAlive)
                .WithValueField(1, 10, name: "MPC", valueProviderCallback: (_) => (uint)this.mpc)
                .WithFlag(11, name: "PAUSE_GNT", valueProviderCallback: (_) => false)
            ;

            Registers.EV_STATUS.Define32(this)
                .WithFlag(0, FieldMode.Read, name: "FINISHED", valueProviderCallback: _ => finishedStatus)
                .WithFlag(1, FieldMode.Read, name: "ILLEGAL_OPCODE", valueProviderCallback: _ => illegalOpcodeStatus)
            ;

            Registers.EV_PENDING.Define32(this)
                .WithFlag(0, out finishedPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "FINISHED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out illegalOpcodePending, FieldMode.Read | FieldMode.WriteOneToClear, name: "ILLEGAL_OPCODE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.EV_ENABLE.Define32(this)
                .WithFlag(0, out finishedEnabled, name: "FINISHED", changeCallback: (_, __) => UpdateInterrupts())
                .WithFlag(1, out illegalOpcodeEnabled, name: "ILLEGAL_OPCODE", changeCallback: (_, __) => UpdateInterrupts())
            ;

            Registers.INSTRUCTION.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => this.engineRam.data[this.mpc])
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.illegalOpcodeStatus)
            {
                this.illegalOpcodePending.Value = true;
            }
            if (this.finishedStatus)
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
            if (this.engineExecution != null) {
                if (this.engineExecution.IsAlive) {
                    this.engineExecution.Abort();
                }
                this.engineExecution = null;
            }
            if (this.powerIsOn != null)
            {
                this.powerIsOn.Value = false;
            }
            RegistersCollection.Reset();
        }

        private readonly Machine machine;

        public GPIO IRQ { get; private set; }

        private byte window;
        private UInt16 mpstart;
        private UInt16 mplen;
        private UInt16 mpresume;
        private int mpc;
        private IFlagRegisterField powerIsOn;
        private IFlagRegisterField pauseRequest;

        private IFlagRegisterField illegalOpcodeEnabled;
        private IFlagRegisterField illegalOpcodePending;
        private bool illegalOpcodeStatus;

        private IFlagRegisterField finishedEnabled;
        private IFlagRegisterField finishedPending;
        private bool finishedStatus;

        private Thread engineExecution;

        private EngineRam engineRam;

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

    public static class ExtensionMethods
    {

        public static byte[] Concat(this byte[] first, byte[] second)
        {
            var buffer = new byte[first.Length + second.Length];
            Buffer.BlockCopy(first, 0, buffer, 0, first.Length);
            Buffer.BlockCopy(second, 0, buffer, first.Length, second.Length);
            return buffer;
        }

        public static byte[] Concat(this byte[] first, byte second)
        {
            var buffer = new byte[first.Length + 1];
            Buffer.BlockCopy(first, 0, buffer, 0, first.Length);
            buffer[buffer.Length - 1] = second;
            return buffer;
        }
    }
}
