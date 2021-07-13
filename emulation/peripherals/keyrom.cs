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

            this.data[0x18] = 0x1c9beae3;
            this.data[0x19] = 0x2aeac875;
            this.data[0x1a] = 0x07c18094;
            this.data[0x1b] = 0x387eff1c;
            this.data[0x1c] = 0x74614282;
            this.data[0x1d] = 0xaffd8152;
            this.data[0x1e] = 0xd871352e;
            this.data[0x1f] = 0xdf3f58bb;
            // Words are swapped on this little-endian system.
            // this.data[0x18] = 0xe3ea9b1c;
            // this.data[0x19] = 0x75c8ea2a;
            // this.data[0x1a] = 0x9480c107;
            // this.data[0x1b] = 0x1cff7e38;
            // this.data[0x1c] = 0x82426174;
            // this.data[0x1d] = 0x5281fdaf;
            // this.data[0x1e] = 0x2e3571d8;
            // this.data[0x1f] = 0xbb583fdf;
            this.lockAddress = 0;
        }

        private uint address;
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
