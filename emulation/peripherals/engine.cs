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
            this.FIELD_MAX = new UInt256(BigInteger.Pow(2, 255) - 19);
            this.c0 = new UInt256(0);
            this.c1 = new UInt256(1);

            // (A-2)/4
            this.c2 = new UInt256(121665);
            this.c3 = this.FIELD_MAX;
            // (A+2)/4
            this.c4 = new UInt256(121666);
            this.c5 = new UInt256(5);
            this.c6 = new UInt256(10);
            this.c7 = new UInt256(20);
            this.c8 = new UInt256(50);
            this.c9 = new UInt256(100);
        }

        public UInt256 FIELD_MAX;
        // private UInt256 UINT_MAX;
        private UInt256 c0;
        private UInt256 c1;
        private UInt256 c2;
        private UInt256 c3;
        private UInt256 c4;
        private UInt256 c5;
        private UInt256 c6;
        private UInt256 c7;
        private UInt256 c8;
        private UInt256 c9;
        private Machine machine;
        private Engine engine;
        private long size;
        public UInt32[] data;
        public long Size { get { return size; } }

        public void WriteDoubleWord(long address, uint value)
        {
            this.data[address / 4] = value;
        }

        public uint ReadDoubleWord(long offset)
        {
            return this.data[offset / 4];
        }

        public UInt256 ReadRegister(ulong rf, ulong register, bool constant)
        {
            // this.Log(LogLevel.Error, "Reading register {0} (const? {1}) from rf {2}", register, constant, rf);
            if (constant)
            {
                switch (register)
                {
                    case 0: return this.c0;
                    case 1: return this.c1;
                    case 2: return this.c2;
                    case 3: return this.c3;
                    case 4: return this.c4;
                    case 5: return this.c5;
                    case 6: return this.c6;
                    case 7: return this.c7;
                    case 8: return this.c8;
                    case 9: return this.c9;
                    default: return new UInt256(0);
                }
            }
            var src_bytes = new byte[32];

            if ((rf >= 16) || (register >= 32))
            {
                throw new Exception(String.Format("Register #{0} or RegisterFile {1} is out of range", register, rf));
            }

            for (ulong i = 0; i < (ulong)src_bytes.Length; i += 4)
            {
                var offset = 0x10000 + (rf * 512) + (register * 32) + i;
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
            return new UInt256(src_bytes);
        }

        public void WriteRegister(long rf, long register, UInt256 value)
        {
            if ((rf >= 16) || (register >= 32))
            {
                throw new Exception(String.Format("Register #{0} or RegisterFile {1} is out of range", register, rf));
            }
            var offset = 0x10000 + (rf * 512) + (register * 32);
            var bytes = value.ToByteArray();
            for (uint i = 0; i < (uint)bytes.Length; i += 4)
            {
                this.data[(offset + i) / 4] = (((uint)bytes[i + 0]) << 0) | (((uint)bytes[i + 1]) << 8) | (((uint)bytes[i + 2]) << 16) | ((uint)(bytes[i + 3]) << 24);

            }
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
                case 9: return "one hundred";
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
            // return "unhandled";
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
            this.shouldEndExecution = false;
            this.startExecution = new SemaphoreSlim(0, 1);
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
            try
            {
                // this.Log(LogLevel.Error, "ENGINE: Beginning execution. Current MPC: 0x{0:X}", this.mpc);
                this.mpc = this.mpstart;
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
                            this.engineRam.WriteRegister(this.window, op.Wd, this.engineRam.ReadRegister(this.window, op.Ra, op.Ca));
                            break;

                        case 1: // PSB
                            this.engineRam.WriteRegister(this.window, op.Wd, this.engineRam.ReadRegister(this.window, op.Rb, op.Cb));
                            break;

                        case 2: // MSK
                            var bit = this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) & 1;
                            if (bit != 0)
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, this.engineRam.ReadRegister(this.window, op.Rb, op.Cb));
                            }
                            else
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, new UInt256(0));
                            }
                            break;

                        case 3: // XOR
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) ^ this.engineRam.ReadRegister(this.window, op.Rb, op.Cb));
                            break;

                        case 4: // NOT
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                ~this.engineRam.ReadRegister(this.window, op.Ra, op.Ca));
                            break;

                        case 5: // ADD
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) + this.engineRam.ReadRegister(this.window, op.Rb, op.Cb));
                            break;

                        case 6: // SUB
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) - this.engineRam.ReadRegister(this.window, op.Rb, op.Cb));
                            break;

                        case 7: // MUL
                            // this.Log(LogLevel.Error, "Ra: {0}  Rb: {1} Wd: {2}", this.engineRam.ReadRegister(this.window, op.Ra, op.Ca), this.engineRam.ReadRegister(this.window, op.Rb, op.Cb),
                            // (this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) * this.engineRam.ReadRegister(this.window, op.Rb, op.Cb)));
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                (this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) * this.engineRam.ReadRegister(this.window, op.Rb, op.Cb))
                                );
                            break;

                        case 8: // TRD
                            if (this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) >= this.engineRam.FIELD_MAX)
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, this.engineRam.FIELD_MAX);
                            }
                            else
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, new UInt256(0));
                            }
                            break;

                        case 9: // BRZ
                            if (this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) == 0)
                            {
                                this.mpc = (ushort)((int)this.mpc + op.Immediate);
                            }
                            break;

                        case 10: // FIN
                            break;

                        case 11: // SHL
                            this.engineRam.WriteRegister(this.window, op.Wd,
                                this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) << 1);
                            break;

                        case 12: // XBT
                            if ((this.engineRam.ReadRegister(this.window, op.Ra, op.Ca) & (new UInt256(1) << 254)) != 0)
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, new UInt256(1));
                            }
                            else
                            {
                                this.engineRam.WriteRegister(this.window, op.Wd, new UInt256(0));
                            }
                            break;

                        default:
                            this.illegalOpcodeStatus = true;
                            throw new Exception("Unhandled opcode");
                    }
                    this.mpc += 1;
                    if (op.Op == 10)
                    {
                        break;
                    }
                    // this.Log(LogLevel.Error, "    (Result) {0}", this.engineRam.ReadRegister(this.window, op.Wd, false).ToString());
                }
            }
            catch (Exception e)
            {
                this.Log(LogLevel.Error, "ENGINE execution exception {0}", e);
            }
            this.finishedStatus = true;
            this.engineExecution = null;
            this.UpdateInterrupts();
            this.finishedStatus = false;
            this.illegalOpcodeStatus = false;
            // this.Log(LogLevel.Error, "ENGINE finished execution");
        }

        private void DefineRegisters()
        {
            Registers.WINDOW.Define(this)
                .WithValueField(0, 4, name: "WINDOW", valueProviderCallback: (_) => { return this.window; }, changeCallback: (_, value) => { this.window = (byte)(value & 0xf); })
            ;

            Registers.MPSTART.Define(this)
                .WithValueField(0, 11, name: "MPSTART", valueProviderCallback: (_) => { return this.mpstart; }, changeCallback: (_, value) => { this.mpstart = (UInt16)(value & 0x7ff); })
            ;

            Registers.MPLEN.Define(this)
                .WithValueField(0, 11, name: "MPLEN", valueProviderCallback: _ => this.mplen, changeCallback: (_, value) => { this.mplen = (UInt16)(value & 0x7ff); })
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
                            this.engineExecution = new Thread(new ThreadStart(this.EngineThread));
                            this.engineExecution.IsBackground = true;
                            this.engineExecution.Start();
                        }
                    }
                })
                ;

            Registers.MPRESUME.Define(this)
                .WithValueField(0, 11, name: "MPRESUME", valueProviderCallback: _ => this.mpresume, changeCallback: (_, value) => { this.mpresume = (UInt16)(value & 0x7ff); })
            ;

            Registers.POWER.Define(this)
                .WithFlag(0, out powerIsOn, FieldMode.Read | FieldMode.Write, name: "ON")
                .WithFlag(1, out pauseRequest, FieldMode.Read | FieldMode.Write, name: "PAUSE_REQ")
            ;

            Registers.STATUS.Define(this)
                .WithFlag(0, name: "RUNNING", valueProviderCallback: (_) => this.engineExecution.IsAlive)
                .WithValueField(1, 10, name: "MPC", valueProviderCallback: (_) => 0)
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
            if (this.powerIsOn != null)
            {
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

        private Thread engineExecution;
        private bool shouldEndExecution;
        private SemaphoreSlim startExecution;

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


    public struct UInt256 : IComparable<UInt256>
    {
        private static readonly UInt256 _zero = new UInt256(new byte[0]);

        // parts are big-endian
        private readonly UInt64 part1;
        private readonly UInt64 part2;
        private readonly UInt64 part3;
        private readonly UInt64 part4;
        private readonly int hashCode;
        private readonly bool notDefault;

        public UInt256(byte[] value)
        {
            // if (value.Length > 32 && !(value.Length == 33 && value[32] == 0))
            // {
            //     throw new Exception(String.Format("value.Length = {0}", value.Length));
            //     // throw new ArgumentOutOfRangeException();
            // }

            if (value.Length < 32)
                value = value.Concat(new byte[32 - value.Length]);

            // read LE parts in reverse order to store in BE
            var part1Bytes = new byte[8];
            var part2Bytes = new byte[8];
            var part3Bytes = new byte[8];
            var part4Bytes = new byte[8];
            Buffer.BlockCopy(value, 0, part4Bytes, 0, 8);
            Buffer.BlockCopy(value, 8, part3Bytes, 0, 8);
            Buffer.BlockCopy(value, 16, part2Bytes, 0, 8);
            Buffer.BlockCopy(value, 24, part1Bytes, 0, 8);

            // convert parts and store
            this.part1 = Bits.ToUInt64(part1Bytes);
            this.part2 = Bits.ToUInt64(part2Bytes);
            this.part3 = Bits.ToUInt64(part3Bytes);
            this.part4 = Bits.ToUInt64(part4Bytes);

            this.hashCode = this.part1.GetHashCode() ^ this.part2.GetHashCode() ^ this.part3.GetHashCode() ^ this.part4.GetHashCode();

            this.notDefault = true;
        }

        private UInt256(UInt64 part1, UInt64 part2, UInt64 part3, UInt64 part4)
        {
            this.part1 = part1;
            this.part2 = part2;
            this.part3 = part3;
            this.part4 = part4;

            this.hashCode = this.part1.GetHashCode() ^ this.part2.GetHashCode() ^ this.part3.GetHashCode() ^ this.part4.GetHashCode();

            this.notDefault = true;
        }

        public UInt256(int value)
            : this(Bits.GetBytes(value))
        {
            if (value < 0)
                throw new ArgumentOutOfRangeException();
        }

        public UInt256(long value)
            : this(Bits.GetBytes(value))
        {
            if (value < 0)
                throw new ArgumentOutOfRangeException();
        }

        public UInt256(uint value)
            : this(Bits.GetBytes(value))
        { }

        public UInt256(ulong value)
            : this(Bits.GetBytes(value))
        { }

        public UInt256(BigInteger value)
            : this(value.ToByteArray())
        {
            if (value < 0)
                throw new ArgumentOutOfRangeException();
        }

        public bool IsDefault { get { return !this.notDefault; } }

        public byte[] ToByteArray()
        {
            var buffer = new byte[32];
            Buffer.BlockCopy(Bits.GetBytes(this.part4), 0, buffer, 0, 8);
            Buffer.BlockCopy(Bits.GetBytes(this.part3), 0, buffer, 8, 8);
            Buffer.BlockCopy(Bits.GetBytes(this.part2), 0, buffer, 16, 8);
            Buffer.BlockCopy(Bits.GetBytes(this.part1), 0, buffer, 24, 8);

            return buffer;
        }

        public BigInteger ToBigInteger()
        {
            // add a trailing zero so that value is always positive
            return new BigInteger(ToByteArray().Concat(0));
        }

        public int CompareTo(UInt256 other)
        {
            if (this == other)
                return 0;
            else if (this < other)
                return -1;
            else if (this > other)
                return +1;

            throw new Exception();
        }

        public static UInt256 Zero
        {
            get { return _zero; }
        }

        public static explicit operator BigInteger(UInt256 value)
        {
            return value.ToBigInteger();
        }

        public static implicit operator UInt256(byte value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(int value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(long value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(sbyte value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(short value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(uint value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(ulong value)
        {
            return new UInt256(value);
        }

        public static implicit operator UInt256(ushort value)
        {
            return new UInt256(value);
        }

        public static bool operator ==(UInt256 left, UInt256 right)
        {
            return left.part1 == right.part1 && left.part2 == right.part2 && left.part3 == right.part3 && left.part4 == right.part4;
        }

        public static bool operator !=(UInt256 left, UInt256 right)
        {
            return !(left == right);
        }

        public static bool operator <(UInt256 left, UInt256 right)
        {
            if (left.part1 < right.part1)
                return true;
            else if (left.part1 == right.part1 && left.part2 < right.part2)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 < right.part3)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 == right.part3 && left.part4 < right.part4)
                return true;

            return false;
        }

        public static bool operator <=(UInt256 left, UInt256 right)
        {
            if (left.part1 < right.part1)
                return true;
            else if (left.part1 == right.part1 && left.part2 < right.part2)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 < right.part3)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 == right.part3 && left.part4 < right.part4)
                return true;

            return left == right;
        }

        public static bool operator >(UInt256 left, UInt256 right)
        {
            if (left.part1 > right.part1)
                return true;
            else if (left.part1 == right.part1 && left.part2 > right.part2)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 > right.part3)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 == right.part3 && left.part4 > right.part4)
                return true;

            return false;
        }

        public static bool operator >=(UInt256 left, UInt256 right)
        {
            if (left.part1 > right.part1)
                return true;
            else if (left.part1 == right.part1 && left.part2 > right.part2)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 > right.part3)
                return true;
            else if (left.part1 == right.part1 && left.part2 == right.part2 && left.part3 == right.part3 && left.part4 > right.part4)
                return true;

            return left == right;
        }

        // TODO doesn't compare against other numerics
        public override bool Equals(object obj)
        {
            if (!(obj is UInt256))
                return false;

            var other = (UInt256)obj;
            return other.part1 == this.part1 && other.part2 == this.part2 && other.part3 == this.part3 && other.part4 == this.part4;
        }

        public override int GetHashCode()
        {
            return this.hashCode;
        }

        public override string ToString()
        {
            var bytes = this.ToByteArray();
            StringBuilder hex = new StringBuilder(bytes.Length * 2);
            foreach (byte b in bytes.Reverse())
                hex.AppendFormat("{0:x2}", b);
            return hex.ToString();
        }

        public static UInt256 Parse(string value)
        {
            return new UInt256(BigInteger.Parse("0" + value).ToByteArray());
        }

        public static UInt256 Parse(string value, IFormatProvider provider)
        {
            return new UInt256(BigInteger.Parse("0" + value, provider).ToByteArray());
        }

        public static UInt256 Parse(string value, NumberStyles style)
        {
            return new UInt256(BigInteger.Parse("0" + value, style).ToByteArray());
        }

        public static UInt256 Parse(string value, NumberStyles style, IFormatProvider provider)
        {
            return new UInt256(BigInteger.Parse("0" + value, style, provider).ToByteArray());
        }

        public static double Log(UInt256 value, double baseValue)
        {
            return BigInteger.Log(value.ToBigInteger(), baseValue);
        }

        public static UInt256 operator %(UInt256 dividend, UInt256 divisor)
        {
            return new UInt256(dividend.ToBigInteger() % divisor.ToBigInteger());
        }

        public static UInt256 Pow(UInt256 value, int exponent)
        {
            return new UInt256(BigInteger.Pow(value.ToBigInteger(), exponent));
        }

        public static UInt256 operator *(UInt256 left, UInt256 right)
        {
            return new UInt256((left.ToBigInteger() * right.ToBigInteger()) % ((BigInteger.Pow(2, 255) - 19)));
        }

        public static UInt256 operator >>(UInt256 value, int shift)
        {
            return new UInt256(value.ToBigInteger() >> shift);
        }

        public static UInt256 operator |(UInt256 left, UInt256 right)
        {
            return new UInt256(left.ToBigInteger() | right.ToBigInteger());
        }

        public static UInt256 operator ^(UInt256 left, UInt256 right)
        {
            return new UInt256(left.ToBigInteger() ^ right.ToBigInteger());
        }

        public static UInt256 operator <<(UInt256 value, int shift)
        {
            return new UInt256(value.ToBigInteger() << shift);
        }

        public static UInt256 operator /(UInt256 dividend, UInt256 divisor)
        {
            return new UInt256(dividend.ToBigInteger() / divisor.ToBigInteger());
        }

        public static UInt256 operator &(UInt256 dividend, UInt256 divisor)
        {
            return new UInt256(dividend.ToBigInteger() & divisor.ToBigInteger());
        }

        public static UInt256 operator +(UInt256 dividend, UInt256 divisor)
        {
            return new UInt256(dividend.ToBigInteger() + divisor.ToBigInteger());
        }

        public static UInt256 operator -(UInt256 dividend, UInt256 divisor)
        {
            if (dividend < divisor)
            {
                return new UInt256((BigInteger.Pow(2, 255) - 1) - (divisor.ToBigInteger() - dividend.ToBigInteger()));
            }
            return new UInt256(dividend.ToBigInteger() - divisor.ToBigInteger());
        }

        public static UInt256 operator ~(UInt256 value)
        {
            return new UInt256(~value.part1, ~value.part2, ~value.part3, ~value.part4);
        }

        public static UInt256 DivRem(UInt256 dividend, UInt256 divisor, out UInt256 remainder)
        {
            BigInteger remainderBigInt;
            var result = new UInt256(BigInteger.DivRem(dividend.ToBigInteger(), divisor.ToBigInteger(), out remainderBigInt));
            remainder = new UInt256(remainderBigInt);
            return result;
        }

        public static explicit operator double(UInt256 value)
        {
            return (double)value.ToBigInteger();
        }
    }

    public class Bits
    {
        private static readonly bool isLE = BitConverter.IsLittleEndian;

        public static byte[] GetBytes(Int16 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(UInt16 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytesBE(UInt16 value)
        {
            return OrderBE(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(Int32 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(UInt32 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytesBE(UInt32 value)
        {
            return OrderBE(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(Int64 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(UInt64 value)
        {
            return Order(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytesBE(UInt64 value)
        {
            return OrderBE(BitConverter.GetBytes(value));
        }

        public static byte[] GetBytes(UInt256 value)
        {
            return value.ToByteArray();
        }

        public static string ToString(byte[] value)
        {
            return BitConverter.ToString(Order(value));
        }

        public static UInt16 ToUInt16(byte[] value)
        {
            return BitConverter.ToUInt16(Order(value), startIndex: 0);
        }

        public static UInt16 ToUInt16BE(byte[] value)
        {
            return BitConverter.ToUInt16(OrderBE(value), startIndex: 0);
        }

        public static UInt32 ToUInt32(byte[] value)
        {
            return BitConverter.ToUInt32(Order(value), startIndex: 0);
        }

        public static UInt64 ToUInt64(byte[] value)
        {
            return BitConverter.ToUInt64(Order(value), startIndex: 0);
        }

        public static UInt64 ToUInt64(byte[] value, int startIndex)
        {
            return BitConverter.ToUInt64(Order(value), startIndex);
        }

        public static UInt256 ToUInt256(byte[] value)
        {
            return new UInt256(value);
        }

        public static byte[] Order(byte[] value)
        {
            return isLE ? value : value.Reverse().ToArray();
        }

        public static byte[] OrderBE(byte[] value)
        {
            return isLE ? value.Reverse().ToArray() : value;
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
