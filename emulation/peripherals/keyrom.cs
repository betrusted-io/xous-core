//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous.Betrusted
{
    public class Keyrom : IDoubleWordPeripheral, IProvidesRegisterCollection<DoubleWordRegisterCollection>, IKnownSize
    {
        public Keyrom(Machine machine)
        {
            RegistersCollection = new DoubleWordRegisterCollection(this);
            this.address = 0;
            this.data = new UInt32[256];
            this.locked = new bool[256];
            Reset();
            DefineRegisters();
        }

        private void DefineRegisters()
        {
            Registers.ADDRESS.Define(this)
                .WithValueField(0, 8, writeCallback: (_, value) => { this.address = value; }, valueProviderCallback: _ => { return this.address; })
            ;

            Registers.DATA.Define(this)
                .WithValueField(0, 32, valueProviderCallback: _ => { if (this.locked[this.address]) { return 0xaa; } return this.data[this.address]; })
            ;

            Registers.LOCKADDR.Define(this)
                .WithValueField(0, 8, name: "LOCKADDR", changeCallback: (_, address) =>
                {
                    this.locked[address] = true;
                })
            ;
            Registers.LOCKSTAT.Define(this)
                .WithFlag(0, name: "LOCKSTAT", valueProviderCallback: _ => { return this.locked[this.address]; })
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

        public long Size { get { return 4096; } }
        public DoubleWordRegisterCollection RegistersCollection { get; private set; }

        public void Reset()
        {
            this.address = 0;
            for (var i = 0; i < this.locked.Length; i++)
            {
                this.locked[i] = false;
            }

            this.data[0x0] = 0x9516051b;
            this.data[0x1] = 0x315aa513;
            this.data[0x2] = 0x28417c6;
            this.data[0x3] = 0x3c6818d6;
            this.data[0x4] = 0x52d8972b;
            this.data[0x5] = 0x10e5b388;
            this.data[0x6] = 0xa2177e24;
            this.data[0x7] = 0xf84ff81f;
            this.data[0x8] = 0xf2460da1;
            this.data[0x9] = 0xf86d51b5;
            this.data[0xa] = 0x4e279ae7;
            this.data[0xb] = 0x224a7158;
            this.data[0xc] = 0x4676edb1;
            this.data[0xd] = 0x297db43b;
            this.data[0xe] = 0xeff3cd32;
            this.data[0xf] = 0xe2f39301;
            this.data[0x10] = 0x560e91d3;
            this.data[0x11] = 0xe9439e5e;
            this.data[0x12] = 0xa600629c;
            this.data[0x13] = 0x3276f5b8;
            this.data[0x14] = 0xcca5ba59;
            this.data[0x15] = 0x9cbae96e;
            this.data[0x16] = 0x96899a9c;
            this.data[0x17] = 0xa92c5624;
            this.data[0x18] = 0x1c9beae3;
            this.data[0x19] = 0x2aeac875;
            this.data[0x1a] = 0x7c18094;
            this.data[0x1b] = 0x387eff1c;
            this.data[0x1c] = 0x74614282;
            this.data[0x1d] = 0xaffd8152;
            this.data[0x1e] = 0xd871352e;
            this.data[0x1f] = 0xdf3f58bb;
            this.data[0x20] = 0x0;
            this.data[0x21] = 0x0;
            this.data[0x22] = 0x0;
            this.data[0x23] = 0x0;
            this.data[0x24] = 0x0;
            this.data[0x25] = 0x0;
            this.data[0x26] = 0x0;
            this.data[0x27] = 0x0;
            this.data[0x28] = 0xf2460da1;
            this.data[0x29] = 0xf86d51b5;
            this.data[0x2a] = 0x4e279ae7;
            this.data[0x2b] = 0x224a7158;
            this.data[0x2c] = 0x4676edb1;
            this.data[0x2d] = 0x297db43b;
            this.data[0x2e] = 0xeff3cd32;
            this.data[0x2f] = 0xe2f39301;
            this.data[0x30] = 0x0;
            this.data[0x31] = 0x0;
            this.data[0x32] = 0x0;
            this.data[0x33] = 0x0;
            this.data[0x34] = 0x0;
            this.data[0x35] = 0x0;
            this.data[0x36] = 0x0;
            this.data[0x37] = 0x0;
            this.data[0x38] = 0x0;
            this.data[0x39] = 0x0;
            this.data[0x3a] = 0x0;
            this.data[0x3b] = 0x0;
            this.data[0x3c] = 0x0;
            this.data[0x3d] = 0x0;
            this.data[0x3e] = 0x0;
            this.data[0x3f] = 0x0;
            this.data[0x40] = 0x0;
            this.data[0x41] = 0x0;
            this.data[0x42] = 0x0;
            this.data[0x43] = 0x0;
            this.data[0x44] = 0x0;
            this.data[0x45] = 0x0;
            this.data[0x46] = 0x0;
            this.data[0x47] = 0x0;
            this.data[0x48] = 0x0;
            this.data[0x49] = 0x0;
            this.data[0x4a] = 0x0;
            this.data[0x4b] = 0x0;
            this.data[0x4c] = 0x0;
            this.data[0x4d] = 0x0;
            this.data[0x4e] = 0x0;
            this.data[0x4f] = 0x0;
            this.data[0x50] = 0x0;
            this.data[0x51] = 0x0;
            this.data[0x52] = 0x0;
            this.data[0x53] = 0x0;
            this.data[0x54] = 0x0;
            this.data[0x55] = 0x0;
            this.data[0x56] = 0x0;
            this.data[0x57] = 0x0;
            this.data[0x58] = 0x0;
            this.data[0x59] = 0x0;
            this.data[0x5a] = 0x0;
            this.data[0x5b] = 0x0;
            this.data[0x5c] = 0x0;
            this.data[0x5d] = 0x0;
            this.data[0x5e] = 0x0;
            this.data[0x5f] = 0x0;
            this.data[0x60] = 0x0;
            this.data[0x61] = 0x0;
            this.data[0x62] = 0x0;
            this.data[0x63] = 0x0;
            this.data[0x64] = 0x0;
            this.data[0x65] = 0x0;
            this.data[0x66] = 0x0;
            this.data[0x67] = 0x0;
            this.data[0x68] = 0x0;
            this.data[0x69] = 0x0;
            this.data[0x6a] = 0x0;
            this.data[0x6b] = 0x0;
            this.data[0x6c] = 0x0;
            this.data[0x6d] = 0x0;
            this.data[0x6e] = 0x0;
            this.data[0x6f] = 0x0;
            this.data[0x70] = 0x0;
            this.data[0x71] = 0x0;
            this.data[0x72] = 0x0;
            this.data[0x73] = 0x0;
            this.data[0x74] = 0x0;
            this.data[0x75] = 0x0;
            this.data[0x76] = 0x0;
            this.data[0x77] = 0x0;
            this.data[0x78] = 0x0;
            this.data[0x79] = 0x0;
            this.data[0x7a] = 0x0;
            this.data[0x7b] = 0x0;
            this.data[0x7c] = 0x0;
            this.data[0x7d] = 0x0;
            this.data[0x7e] = 0x0;
            this.data[0x7f] = 0x0;
            this.data[0x80] = 0x0;
            this.data[0x81] = 0x0;
            this.data[0x82] = 0x0;
            this.data[0x83] = 0x0;
            this.data[0x84] = 0x0;
            this.data[0x85] = 0x0;
            this.data[0x86] = 0x0;
            this.data[0x87] = 0x0;
            this.data[0x88] = 0x0;
            this.data[0x89] = 0x0;
            this.data[0x8a] = 0x0;
            this.data[0x8b] = 0x0;
            this.data[0x8c] = 0x0;
            this.data[0x8d] = 0x0;
            this.data[0x8e] = 0x0;
            this.data[0x8f] = 0x0;
            this.data[0x90] = 0x0;
            this.data[0x91] = 0x0;
            this.data[0x92] = 0x0;
            this.data[0x93] = 0x0;
            this.data[0x94] = 0x0;
            this.data[0x95] = 0x0;
            this.data[0x96] = 0x0;
            this.data[0x97] = 0x0;
            this.data[0x98] = 0x0;
            this.data[0x99] = 0x0;
            this.data[0x9a] = 0x0;
            this.data[0x9b] = 0x0;
            this.data[0x9c] = 0x0;
            this.data[0x9d] = 0x0;
            this.data[0x9e] = 0x0;
            this.data[0x9f] = 0x0;
            this.data[0xa0] = 0x0;
            this.data[0xa1] = 0x0;
            this.data[0xa2] = 0x0;
            this.data[0xa3] = 0x0;
            this.data[0xa4] = 0x0;
            this.data[0xa5] = 0x0;
            this.data[0xa6] = 0x0;
            this.data[0xa7] = 0x0;
            this.data[0xa8] = 0x0;
            this.data[0xa9] = 0x0;
            this.data[0xaa] = 0x0;
            this.data[0xab] = 0x0;
            this.data[0xac] = 0x0;
            this.data[0xad] = 0x0;
            this.data[0xae] = 0x0;
            this.data[0xaf] = 0x0;
            this.data[0xb0] = 0x0;
            this.data[0xb1] = 0x0;
            this.data[0xb2] = 0x0;
            this.data[0xb3] = 0x0;
            this.data[0xb4] = 0x0;
            this.data[0xb5] = 0x0;
            this.data[0xb6] = 0x0;
            this.data[0xb7] = 0x0;
            this.data[0xb8] = 0x0;
            this.data[0xb9] = 0x0;
            this.data[0xba] = 0x0;
            this.data[0xbb] = 0x0;
            this.data[0xbc] = 0x0;
            this.data[0xbd] = 0x0;
            this.data[0xbe] = 0x0;
            this.data[0xbf] = 0x0;
            this.data[0xc0] = 0x0;
            this.data[0xc1] = 0x0;
            this.data[0xc2] = 0x0;
            this.data[0xc3] = 0x0;
            this.data[0xc4] = 0x0;
            this.data[0xc5] = 0x0;
            this.data[0xc6] = 0x0;
            this.data[0xc7] = 0x0;
            this.data[0xc8] = 0x0;
            this.data[0xc9] = 0x0;
            this.data[0xca] = 0x0;
            this.data[0xcb] = 0x0;
            this.data[0xcc] = 0x0;
            this.data[0xcd] = 0x0;
            this.data[0xce] = 0x0;
            this.data[0xcf] = 0x0;
            this.data[0xd0] = 0x0;
            this.data[0xd1] = 0x0;
            this.data[0xd2] = 0x0;
            this.data[0xd3] = 0x0;
            this.data[0xd4] = 0x0;
            this.data[0xd5] = 0x0;
            this.data[0xd6] = 0x0;
            this.data[0xd7] = 0x0;
            this.data[0xd8] = 0x0;
            this.data[0xd9] = 0x0;
            this.data[0xda] = 0x0;
            this.data[0xdb] = 0x0;
            this.data[0xdc] = 0x0;
            this.data[0xdd] = 0x0;
            this.data[0xde] = 0x0;
            this.data[0xdf] = 0x0;
            this.data[0xe0] = 0x0;
            this.data[0xe1] = 0x0;
            this.data[0xe2] = 0x0;
            this.data[0xe3] = 0x0;
            this.data[0xe4] = 0x0;
            this.data[0xe5] = 0x0;
            this.data[0xe6] = 0x0;
            this.data[0xe7] = 0x0;
            this.data[0xe8] = 0x0;
            this.data[0xe9] = 0x0;
            this.data[0xea] = 0x0;
            this.data[0xeb] = 0x0;
            this.data[0xec] = 0x0;
            this.data[0xed] = 0x0;
            this.data[0xee] = 0x0;
            this.data[0xef] = 0x0;
            this.data[0xf0] = 0x0;
            this.data[0xf1] = 0x0;
            this.data[0xf2] = 0x0;
            this.data[0xf3] = 0x0;
            this.data[0xf4] = 0x0;
            this.data[0xf5] = 0x0;
            this.data[0xf6] = 0x0;
            this.data[0xf7] = 0x0;
            this.data[0xf8] = 0xe01133bd;
            this.data[0xf9] = 0xc0ea362;
            this.data[0xfa] = 0x9d48a555;
            this.data[0xfb] = 0x947a820;
            this.data[0xfc] = 0x0;
            this.data[0xfd] = 0x0;
            this.data[0xfe] = 0x0;
            this.data[0xff] = 0x1000008;

            this.lockAddress = 0;
        }

        private ulong address;
        private UInt32[] data;
        private bool[] locked;
        private uint lockAddress;

        private enum Registers
        {
            ADDRESS = 0x00,
            DATA = 0x04,
            LOCKADDR = 0x08,
            LOCKSTAT = 0x0C,
        }
    }
}
