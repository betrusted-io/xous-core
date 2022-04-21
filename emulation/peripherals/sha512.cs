//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Collections.Generic;
using System.Linq;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    using System;
    using System.Security.Cryptography;
    using Antmicro.Renode.Core;
    using Antmicro.Renode.Core.Structure.Registers;
    using Antmicro.Renode.Logging;
    using Antmicro.Renode.Peripherals.Bus;
    public class Sha512Writer : IDoubleWordPeripheral, IBytePeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Sha512Writer(Machine machine, Sha512 sha512)
        {
            this.machine = machine;
            this.sha512 = sha512;
        }

        private Machine machine;
        private Sha512 sha512;
        public long Size { get { return 4096; } }

        public void WriteDoubleWord(long offset, uint value)
        {
            if (offset != 0)
            {
                this.Log(LogLevel.Error, "Adding word 0x{0:X} (at address 0x{1:X}) to hash", value, offset);
            }
            this.sha512.add32ToHash(value, (uint)offset);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public byte ReadByte(long offset)
        {
            this.Log(LogLevel.Error, "Reading byte from 0x{0:X}", offset);
            return 0;
        }

        public void WriteByte(long offset, byte value)
        {
            this.Log(LogLevel.Error, "Writing byte value 0x{0:X} to 0x{0:X}", value, offset);
            this.sha512.add8ToHash(value, (uint)offset);
        }

        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Reset()
        {
        }

    }
    public class Sha512 : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Sha512(Machine machine, uint memAddr, uint memSize)
        {
            this.machine = machine;
            var sha512Writer = new Sha512Writer(machine, this);
            machine.SystemBus.Register(sha512Writer, new BusRangeRegistration(memAddr, memSize));

            RegistersCollection = new DoubleWordRegisterCollection(this);
            this.IRQ = new GPIO();
            this.sha512 = new SHA512Managed();
            Reset();
            DefineRegisters();
        }

        public void add32ToHash(UInt32 encounteredValue, uint address)
        {
            var inputBuffer = new byte[4];
            if (!this.inputIsSwapped.Value)
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
            }
            if (this.usingSha256.Value)
            {
                foreach (byte b in inputBuffer)
                {
                    this.Sha512_256Backing.Add(b);
                }
            }
            else
            {
                this.sha512.TransformBlock(inputBuffer, 0, inputBuffer.Length, null, 0);
            }
            this.digestedLength += (ulong)inputBuffer.Length * 8;
        }

        public void add8ToHash(byte encounteredValue, uint address)
        {
            var inputBuffer = new byte[1];
            inputBuffer[0] = encounteredValue;
            if (this.usingSha256.Value)
            {
                this.Sha512_256Backing.Add(inputBuffer[0]);
            }
            else
            {
                this.sha512.TransformBlock(inputBuffer, 0, inputBuffer.Length, null, 0);
            }
            this.digestedLength += (ulong)inputBuffer.Length * 8;
        }

        private uint ResultAtOffset(int offset)
        {
            // this.Log(LogLevel.Error, "Hash is: {0}  Offset[{1}]: 0x{2:X}", this.sha512, offset, this.sha512.Hash[offset + 0]);
            if (usingSha256.Value && offset >= 32)
            {
                offset -= 32;
            }

            // Using sha 512
            var arr = this.sha512.Hash;
            if (this.usingSha256.Value && (this.Sha512_256Result != null))
            {
                arr = this.Sha512_256Result.Hash;
            }

            if (outputIsSwapped.Value)
            {
                return ((uint)arr[offset + 0] << 0)
                | ((uint)arr[offset + 1] << 8)
                | ((uint)arr[offset + 2] << 16)
                | ((uint)arr[offset + 3] << 24);
            }
            else
            {
                return ((uint)arr[offset + 3] << 0)
                | ((uint)arr[offset + 2] << 8)
                | ((uint)arr[offset + 1] << 16)
                | ((uint)arr[offset + 0] << 24);
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
                .WithFlag(3, out usingSha256, FieldMode.Read | FieldMode.Write, name: "SELECT_256")
                .WithFlag(4, FieldMode.Write, name: "RESET", writeCallback: (_, doit) => { if (doit) Reset(); })
            ;

            Registers.COMMAND.Define(this)
                .WithFlag(0, name: "HASH_START", writeCallback: (_, value) =>
                {
                    if (value)
                    {
                        if (this.usingSha256.Value)
                        {
                            // this.sha512 = SHA512Managed.Create("SHA512/256");
                            this.Sha512_256Backing = new List<byte>();
                            this.Sha512_256Result = null;
                        }
                        else
                        {
                            this.sha512 = SHA512Managed.Create("SHA512");
                            this.sha512.Initialize();
                        }
                        this.digestedLength = 0;
                    }
                })
                .WithFlag(1, name: "HASH_PROCESS", writeCallback: (_, value) =>
                {
                    if (value)
                    {
                        if (this.usingSha256.Value)
                        {
                            this.Sha512_256Result = new SHA512_256Managed(this.Sha512_256Backing.ToArray());
                            this.Sha512_256Backing = null;
                        }
                        else
                        {
                            var dummy = new byte[0];
                            this.sha512.TransformFinalBlock(dummy, 0, 0);
                        }

                        sha512DoneStatus = true;
                        UpdateInterrupts();
                        sha512DoneStatus = false;
                    }
                })
            ;

            Registers.DIGEST00.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(0))
            ;
            Registers.DIGEST01.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(4))
            ;
            Registers.DIGEST10.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(8))
            ;
            Registers.DIGEST11.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(12))
            ;
            Registers.DIGEST20.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(16))
            ;
            Registers.DIGEST21.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(20))
            ;
            Registers.DIGEST30.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(24))
            ;
            Registers.DIGEST31.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(28))
            ;
            Registers.DIGEST40.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(32))
            ;
            Registers.DIGEST41.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(36))
            ;
            Registers.DIGEST50.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(40))
            ;
            Registers.DIGEST51.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(44))
            ;
            Registers.DIGEST60.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(48))
            ;
            Registers.DIGEST61.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(52))
            ;
            Registers.DIGEST70.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(56))
            ;
            Registers.DIGEST71.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => ResultAtOffset(60))
            ;
            Registers.MSG_LENGTH1.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => (uint)(this.digestedLength >> 32))
            ;
            Registers.MSG_LENGTH0.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => (uint)this.digestedLength)
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
                .WithValueField(0, 32, valueProviderCallback: _ => 0)
            ;
        }

        private void UpdateInterrupts()
        {
            if (this.errValidStatus)
            {
                this.errValidPending.Value = true;
            }
            if (this.fifoFullStatus)
            {
                this.fifoFullPending.Value = true;
            }
            if (this.sha512DoneStatus)
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
            this.sha512.Initialize();
            this.digestedLength = 0;
            RegistersCollection.Reset();
        }

        private readonly Machine machine;

        private SHA512 sha512;
        public GPIO IRQ { get; private set; }

        private List<byte> Sha512_256Backing;
        private SHA512_256Managed Sha512_256Result;

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

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    using System.Globalization;
    using System.Diagnostics.Contracts;
    using System.Security.Cryptography;
    using Antmicro.Renode.Core;
    using Antmicro.Renode.Core.Structure.Registers;
    using Antmicro.Renode.Logging;
    using Antmicro.Renode.Peripherals.Bus;
}
public class SHA512_256Managed
{
    UInt64[] H0Sha512_256;
    UInt64[] K512;
    public byte[] Hash;

    //
    // public constructors
    //

    public SHA512_256Managed(byte[] plaintext)
    {
        // These eight 64-bit words are obtained from GenerateInitialHashSha512t(256)
        // They are manually added to the array here due to bugs in the Renode C# parser. Attemtping
        // to use a normal array will give an error such as:
        // (SoC) i @peripherals/sha512.cs
        //     Errors during compilation or loading:
        //     Could not compile assembly: D:\Code\Xous\Core\emulation\peripherals\sha512.cs(407,5):
        //     14:56:16.2452 [INFO] Including script: D:\Code\Xous\Core\emulation\peripherals\sha512.cs (2)
        // (SoC)
        this.H0Sha512_256 = new UInt64[8];
        this.H0Sha512_256[0] = 0x22312194fc2bf72c;
        this.H0Sha512_256[1] = 0x9f555fa3c84c64c2;
        this.H0Sha512_256[2] = 0x2393b86b6f53b151;
        this.H0Sha512_256[3] = 0x963877195940eabd;
        this.H0Sha512_256[4] = 0x96283ee2a88effe3;
        this.H0Sha512_256[5] = 0xbe5e1e2553863992;
        this.H0Sha512_256[6] = 0x2b0199fc2c85b8aa;
        this.H0Sha512_256[7] = 0x0eb72ddc81c52ca2;
        // The eighty 64-bit words in the array K512 are used in Sha-384, Sha-512, Sha-512/224, Sha-512/256.
        // They are obtained by taking the first 64 bits of the fractional
        // parts of the cube roots of the first eighty primes.
        // They are manually added to the array here due to bugs in the Renode C# parser.
        this.K512 = new UInt64[80];
        this.K512[0] = 0x428a2f98d728ae22;
        this.K512[1] = 0x7137449123ef65cd;
        this.K512[2] = 0xb5c0fbcfec4d3b2f;
        this.K512[3] = 0xe9b5dba58189dbbc;
        this.K512[4] = 0x3956c25bf348b538;
        this.K512[5] = 0x59f111f1b605d019;
        this.K512[6] = 0x923f82a4af194f9b;
        this.K512[7] = 0xab1c5ed5da6d8118;
        this.K512[8] = 0xd807aa98a3030242;
        this.K512[9] = 0x12835b0145706fbe;
        this.K512[10] = 0x243185be4ee4b28c;
        this.K512[11] = 0x550c7dc3d5ffb4e2;
        this.K512[12] = 0x72be5d74f27b896f;
        this.K512[13] = 0x80deb1fe3b1696b1;
        this.K512[14] = 0x9bdc06a725c71235;
        this.K512[15] = 0xc19bf174cf692694;
        this.K512[16] = 0xe49b69c19ef14ad2;
        this.K512[17] = 0xefbe4786384f25e3;
        this.K512[18] = 0x0fc19dc68b8cd5b5;
        this.K512[19] = 0x240ca1cc77ac9c65;
        this.K512[20] = 0x2de92c6f592b0275;
        this.K512[21] = 0x4a7484aa6ea6e483;
        this.K512[22] = 0x5cb0a9dcbd41fbd4;
        this.K512[23] = 0x76f988da831153b5;
        this.K512[24] = 0x983e5152ee66dfab;
        this.K512[25] = 0xa831c66d2db43210;
        this.K512[26] = 0xb00327c898fb213f;
        this.K512[27] = 0xbf597fc7beef0ee4;
        this.K512[28] = 0xc6e00bf33da88fc2;
        this.K512[29] = 0xd5a79147930aa725;
        this.K512[30] = 0x06ca6351e003826f;
        this.K512[31] = 0x142929670a0e6e70;
        this.K512[32] = 0x27b70a8546d22ffc;
        this.K512[33] = 0x2e1b21385c26c926;
        this.K512[34] = 0x4d2c6dfc5ac42aed;
        this.K512[35] = 0x53380d139d95b3df;
        this.K512[36] = 0x650a73548baf63de;
        this.K512[37] = 0x766a0abb3c77b2a8;
        this.K512[38] = 0x81c2c92e47edaee6;
        this.K512[39] = 0x92722c851482353b;
        this.K512[40] = 0xa2bfe8a14cf10364;
        this.K512[41] = 0xa81a664bbc423001;
        this.K512[42] = 0xc24b8b70d0f89791;
        this.K512[43] = 0xc76c51a30654be30;
        this.K512[44] = 0xd192e819d6ef5218;
        this.K512[45] = 0xd69906245565a910;
        this.K512[46] = 0xf40e35855771202a;
        this.K512[47] = 0x106aa07032bbd1b8;
        this.K512[48] = 0x19a4c116b8d2d0c8;
        this.K512[49] = 0x1e376c085141ab53;
        this.K512[50] = 0x2748774cdf8eeb99;
        this.K512[51] = 0x34b0bcb5e19b48a8;
        this.K512[52] = 0x391c0cb3c5c95a63;
        this.K512[53] = 0x4ed8aa4ae3418acb;
        this.K512[54] = 0x5b9cca4f7763e373;
        this.K512[55] = 0x682e6ff3d6b2b8a3;
        this.K512[56] = 0x748f82ee5defb2fc;
        this.K512[57] = 0x78a5636f43172f60;
        this.K512[58] = 0x84c87814a1f0ab72;
        this.K512[59] = 0x8cc702081a6439ec;
        this.K512[60] = 0x90befffa23631e28;
        this.K512[61] = 0xa4506cebde82bde9;
        this.K512[62] = 0xbef9a3f7b2c67915;
        this.K512[63] = 0xc67178f2e372532b;
        this.K512[64] = 0xca273eceea26619c;
        this.K512[65] = 0xd186b8c721c0c207;
        this.K512[66] = 0xeada7dd6cde0eb1e;
        this.K512[67] = 0xf57d4f7fee6ed178;
        this.K512[68] = 0x06f067aa72176fba;
        this.K512[69] = 0x0a637dc5a2c898a6;
        this.K512[70] = 0x113f9804bef90dae;
        this.K512[71] = 0x1b710b35131c471b;
        this.K512[72] = 0x28db77f523047d84;
        this.K512[73] = 0x32caab7b40c72493;
        this.K512[74] = 0x3c9ebe0a15c9bebc;
        this.K512[75] = 0x431d67c49c100d4c;
        this.K512[76] = 0x4cc5d4becb3e42b6;
        this.K512[77] = 0x597f299cfc657e2a;
        this.K512[78] = 0x5fcb6fab3ad6faec;
        this.K512[79] = 0x6c44198c4a475817;
        this.Hash = Sha512Algorithm(plaintext, this.H0Sha512_256, 256);
    }

    byte[] Sha512Algorithm(byte[] plaintext, UInt64[] H0, int numberBits)
    {
        Block1024[] blocks = ConvertPaddedMessageToBlock1024Array(PadPlainText1024(plaintext));

        // Define the hash variable and set its initial values.
        UInt64[] H = H0;

        for (int i = 0; i < blocks.Length; i++)
        {
            UInt64[] W = CreateMessageScheduleSha512(blocks[i]);

            // Set the working variables a,...,h to the current hash values.
            UInt64 a = H[0];
            UInt64 b = H[1];
            UInt64 c = H[2];
            UInt64 d = H[3];
            UInt64 e = H[4];
            UInt64 f = H[5];
            UInt64 g = H[6];
            UInt64 h = H[7];

            for (int t = 0; t < 80; t++)
            {
                UInt64 T1 = h + Sigma1_512(e) + Ch(e, f, g) + this.K512[t] + W[t];
                UInt64 T2 = Sigma0_512(a) + Maj(a, b, c);
                h = g;
                g = f;
                f = e;
                e = d + T1;
                d = c;
                c = b;
                b = a;
                a = T1 + T2;
            }

            // Update the current value of the hash H after processing block i.
            H[0] += a;
            H[1] += b;
            H[2] += c;
            H[3] += d;
            H[4] += e;
            H[5] += f;
            H[6] += g;
            H[7] += h;
        }

        // Concatenate all the Word64 Hash Values
        byte[] hash = ShaUtilities.Word64ArrayToByteArray(H);

        // The number of bytes in the final output hash 
        int numberBytes = numberBits / 8;
        byte[] truncatedHash = new byte[numberBytes];
        Array.Copy(hash, truncatedHash, numberBytes);

        return truncatedHash;
    }


    static UInt32 Maj(UInt32 x, UInt32 y, UInt32 z)
    {
        return (x & y) ^ (x & z) ^ (y & z);
    }

    static UInt64 Maj(UInt64 x, UInt64 y, UInt64 z)
    {
        return (x & y) ^ (x & z) ^ (y & z);
    }

    static UInt32 Ch(UInt32 x, UInt32 y, UInt32 z)
    {
        return (x & y) ^ (~x & z);
    }

    static UInt64 Ch(UInt64 x, UInt64 y, UInt64 z)
    {
        return (x & y) ^ (~x & z);
    }

    static UInt64 Sigma0_512(UInt64 x)
    {
        return RotR(28, x) ^ RotR(34, x) ^ RotR(39, x);
    }

    static UInt64 Sigma1_512(UInt64 x)
    {
        return RotR(14, x) ^ RotR(18, x) ^ RotR(41, x);
    }

    static UInt64 sigma0_512(UInt64 x)
    {
        return RotR(1, x) ^ RotR(8, x) ^ ShR(7, x);
    }

    static UInt64 sigma1_512(UInt64 x)
    {
        return RotR(19, x) ^ RotR(61, x) ^ ShR(6, x);
    }


    static UInt64 ShR(int n, UInt64 x)
    {
        // should have 0 <= n < 64
        return (x >> n);
    }
    static UInt32 RotR(int n, UInt32 x)
    {
        // should have 0 <= n < 32
        return (x >> n) | (x << 32 - n);
    }

    static UInt64 RotR(int n, UInt64 x)
    {
        // should have 0 <= n < 64
        return (x >> n) | (x << 64 - n);
    }
    static byte[] PadPlainText1024(byte[] plaintext)
    {
        // After padding the total bits of the output will be divisible by 1024.
        int numberBits = plaintext.Length * 8;
        int t = (numberBits + 8 + 128) / 1024;

        // Note that 1024 * (t + 1) is the least multiple of 1024 greater than (numberBits + 8 + 128)
        // Therefore the number of zero bits we need to add is
        int k = 1024 * (t + 1) - (numberBits + 8 + 128);

        // Since numberBits % 8 = 0, we know k % 8 = 0. So n = k / 8 is the number of zero bytes to add.
        int n = k / 8;

        List<byte> paddedtext = plaintext.ToList();

        // Start the padding by concatenating 1000_0000 = 0x80 = 128
        paddedtext.Add(0x80);

        // Next add n zero bytes
        for (int i = 0; i < n; i++)
        {
            paddedtext.Add(0);
        }

        // Now add 16 bytes (128 bits) to represent the length of the message in bits.
        // C# does not have 128 bit integer.
        // For now just add 8 zero bytes and then 8 bytes to represent the int
        for (int i = 0; i < 8; i++)
        {
            paddedtext.Add(0);
        }

        byte[] B = BitConverter.GetBytes((ulong)numberBits);
        Array.Reverse(B);

        for (int i = 0; i < B.Length; i++)
        {
            paddedtext.Add(B[i]);
        }

        return paddedtext.ToArray();
    }

    static Block1024[] ConvertPaddedMessageToBlock1024Array(byte[] M)
    {
        // We are assuming M is padded, so the number of bits in M is divisible by 1024 
        int numberBlocks = (M.Length * 8) / 1024;  // same as: M.Length / 128
        Block1024[] blocks = new Block1024[numberBlocks];

        for (int i = 0; i < numberBlocks; i++)
        {
            // First extract the relavant subarray from M
            byte[] B = new byte[128]; // 128 * 8 = 1024

            for (int j = 0; j < 128; j++)
            {
                B[j] = M[i * 128 + j];
            }

            UInt64[] words = ShaUtilities.ByteArrayToWord64Array(B);
            blocks[i] = new Block1024(words);
        }

        return blocks;
    }

    static UInt64[] CreateMessageScheduleSha512(Block1024 block)
    {
        // The message schedule.
        UInt64[] W = new UInt64[80];

        // Prepare the message schedule W.
        // The first 16 words in W are the same as the words of the block.
        // The remaining 80-16 =64 words in W are functions of the previously defined words. 
        for (int t = 0; t < 80; t++)
        {
            if (t < 16)
            {
                W[t] = block.words[t];
            }
            else
            {
                W[t] = sigma1_512(W[t - 2]) + W[t - 7] + sigma0_512(W[t - 15]) + W[t - 16];
            }
        }

        return W;
    }

    class Block1024
    {
        // A Block1024 consists of an array of 16 elements of type Word64.
        public UInt64[] words;

        public Block1024(UInt64[] words)
        {
            if (words.Length == 16)
            {
                this.words = words;
            }
            else
            {
                Console.WriteLine("ERROR: A block must be 16 words");
                this.words = null;
            }
        }
    }

    static class ShaUtilities
    {
        public static UInt64[] ByteArrayToWord64Array(byte[] B)
        {
            // We assume B is not null, is not empty and number elements is divisible by 8
            int numberWords = B.Length / 8; // 8 bytes for each Word32
            UInt64[] word64Array = new UInt64[numberWords];

            for (int i = 0; i < numberWords; i++)
            {
                word64Array[i] = ByteArrayToWord64(B, 8 * i);
            }

            return word64Array;
        }


        public static UInt64 ByteArrayToWord64(byte[] B, int startIndex)
        {
            // We assume: 0 <= startIndex < B. Length, and startIndex + 8 <= B.Length
            UInt64 c = 256;
            UInt64 output = 0;

            for (int i = startIndex; i < startIndex + 8; i++)
            {
                output = output * c + B[i];
            }

            return output;
        }

        // Returns an array of 8 bytes.
        public static byte[] Word64ToByteArray(UInt64 x)
        {
            byte[] b = BitConverter.GetBytes(x);
            Array.Reverse(b);
            return b;
        }
        public static byte[] Word64ArrayToByteArray(UInt64[] words)
        {
            List<byte> b = new List<byte>();

            for (int i = 0; i < words.Length; i++)
            {
                b.AddRange(Word64ToByteArray(words[i]));
            }

            return b.ToArray();
        }

    }
}
