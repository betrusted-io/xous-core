//
// Copyright (c) 2010-2019 Antmicro
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//

using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.SPI;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.SPI.Betrusted
{
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord | AllowedTranslation.WordToDoubleWord)]
    public class BetrustedSpinor : NullRegistrationPointPeripheralContainer<ISPIPeripheral>, IDoubleWordPeripheral, IKnownSize
    {
        public BetrustedSpinor(Machine machine) : base(machine)
        {
            // bbHelper = new BitBangHelper(8, loggingParent: this, outputMsbFirst: true);
            txQueue = new Queue<byte>(256);
            rxQueue = new Queue<byte>(256);
            IRQ = new GPIO();

            var registers = new Dictionary<long, DoubleWordRegister>
            {
                {(long)Registers.Config, new DoubleWordRegister(this)
                    .WithValueField(0, 5, name: "dummy")
                },
                {(long)Registers.DelayConfig, new DoubleWordRegister(this)
                    .WithValueField(0, 5, name: "d")
                    .WithFlag(5, name: "load")
                },
                {(long)Registers.DelayStatus, new DoubleWordRegister(this)
                    .WithValueField(0, 5, name: "q")
                },
                {(long)Registers.Command, new DoubleWordRegister(this)
                    .WithValueField(0, 32, FieldMode.Write, writeCallback: (_, data) => {
                        var wakeup = BitHelper.IsBitSet(data, 0);
                        var exec_cmd = BitHelper.IsBitSet(data, 1);
                        cmdCode = (byte)BitHelper.GetValue(data, 2, 8);
                        hasArg = BitHelper.IsBitSet(data, 10);
                        dummyCycles = (uint)BitHelper.GetValue(data, 11, 5);
                        dataWords = (uint)BitHelper.GetValue(data, 16, 8);
                        lockReads = BitHelper.IsBitSet(data, 24);

                        if (exec_cmd) {
                            // Ensure the device is in OPI mode. This may change due to a system reset.
                            SwitchSpiToOpi();

                            this.Log(LogLevel.Noisy, "Executing command! cmdCode: {0:X2}  hasArg? {1} ({6:X8})  dummyCycles: {2}  dataWords: {3}  txFifo.Count: {4}  rxFifo.Count: {5}",
                                cmdCode, hasArg, dummyCycles, dataWords, txQueue.Count, rxQueue.Count, cmdArg.Value);
                            if (RegisteredPeripheral == null) {
                                    this.Log(LogLevel.Error, "Unable to get SPI chip device!");
                                    return;
                                }

                            // In OPI mode, send the command, followed by the inverse of the command
                            RegisteredPeripheral.Transmit(cmdCode);
                            RegisteredPeripheral.Transmit((byte)~cmdCode);

                            if (dummyCycles == 0) {
                                if (hasArg) {
                                    RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 24));
                                    RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 16));
                                    RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 8));
                                    RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 0));
                                }
                                // Is Write
                                for (var i = 0; i < dataWords; i++) {
                                    RegisteredPeripheral.Transmit((txQueue.Count > 0) ? txQueue.Dequeue() : (byte)0xff);
                                    RegisteredPeripheral.Transmit((txQueue.Count > 0) ? txQueue.Dequeue() : (byte)0xff);
                                }
                            } else {
                                // Is Read

                                // Transmit the dummy data
                                for (var i = 0; i < dummyCycles; i++) {
                                    if (hasArg && (i == 0)) {
                                        RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 24));
                                    } else if (hasArg && (i == 1)) {
                                        RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 16));
                                    } else if (hasArg && (i == 2)) {
                                        RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 8));
                                    } else if (hasArg && (i == 3)) {
                                        RegisteredPeripheral.Transmit((byte)(cmdArg.Value >> 0));
                                    } else {
                                        RegisteredPeripheral.Transmit(0xff);
                                    }
                                }

                                // Receive data back, two transmissions per word since it's in
                                // units of 16 bits.
                                for (var i = 0; i < dataWords; i++) {
                                    rxQueue.Enqueue((byte)(RegisteredPeripheral.Transmit(0xff)));
                                    rxQueue.Enqueue((byte)(RegisteredPeripheral.Transmit(0xff) >> 8));
                                }
                            }

                            RegisteredPeripheral.FinishTransmission();
                        }
                    })
                },
                {(long)Registers.CmdArg, new DoubleWordRegister(this)
                    .WithValueField(0, 32, out cmdArg, FieldMode.Read | FieldMode.Write, name: "cmd_arg")
                },
                {(long)Registers.CmdRbkData, new DoubleWordRegister(this)
                    .WithValueField(0, 32, FieldMode.Read, valueProviderCallback: (_) => HandleRxFifoRead(), name: "cmd_rbk_data")
                },
                {(long)Registers.Status, new DoubleWordRegister(this)
                    .WithFlag(0, name: "wip")
                },
                {(long)Registers.WData, new DoubleWordRegister(this)
                    .WithValueField(0, 16, FieldMode.Write, writeCallback: (_, data) => HandleTxFifoWrite(data), name: "wdata")
                },
                {(long)Registers.EvStatus, new DoubleWordRegister(this)
                    .WithFlag(0, name: "ecc_error")
                },
                {(long)Registers.EvPending, new DoubleWordRegister(this)
                    .WithFlag(0, name: "ecc_error")
                },
                {(long)Registers.EvEnable, new DoubleWordRegister(this)
                    .WithFlag(0, name: "ecc_error")
                },
                {(long)Registers.EccAddress, new DoubleWordRegister(this)
                    .WithValueField(0, 32, name: "ecc_address")
                },
                {(long)Registers.EccStatus, new DoubleWordRegister(this)
                    .WithFlag(0, name: "ecc_error")
                    .WithFlag(1, name: "ecc_overflow")
                },
            };

            registersCollection = new DoubleWordRegisterCollection(this, registers);
            Reset();
        }

        public override void Reset()
        {
            registersCollection.Reset();
            if (RegisteredPeripheral != null) {
                spiIsInOpiMode = false;
                RegisteredPeripheral.Reset();
            }
        }

        private void HandleTxFifoWrite(ulong value)
        {
            txQueue.Enqueue((byte)(value));
            txQueue.Enqueue((byte)(value >> 8));
        }

        private uint HandleRxFifoRead()
        {
            uint ret = 0;
            if (rxQueue.Count > 0)
            {
                ret |= (uint)rxQueue.Dequeue() << 0;
            }
            if (rxQueue.Count > 0)
            {
                ret |= (uint)rxQueue.Dequeue() << 8;
            }
            if (rxQueue.Count > 0)
            {
                ret |= (uint)rxQueue.Dequeue() << 16;
            }
            if (rxQueue.Count > 0)
            {
                ret |= (uint)rxQueue.Dequeue() << 24;
            }
            return ret;
        }

        public uint ReadDoubleWord(long offset)
        {
            return registersCollection.Read(offset);
        }

        [ConnectionRegionAttribute("xip")]
        public uint XipReadDoubleWord(long offset)
        {
            return (RegisteredPeripheral as IDoubleWordPeripheral)?.ReadDoubleWord(offset) ?? 0;
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            registersCollection.Write(offset, value);
        }

        [ConnectionRegionAttribute("xip")]
        public void XipWriteDoubleWord(long offset, uint value)
        {
            (RegisteredPeripheral as IDoubleWordPeripheral)?.WriteDoubleWord(offset, value);
        }

        public override void Register(ISPIPeripheral peripheral, NullRegistrationPoint registrationPoint)
        {
            peripheral.Reset();
            base.Register(peripheral, registrationPoint);
            SwitchSpiToOpi();
        }

        private void SwitchSpiToOpi()
        {
            if (spiIsInOpiMode) {
                return;
            }

            // Wake up from deep sleep
            byte[] wakeup = new byte[] { 0xab };

            // WREN
            byte[] wren = new byte[] { 0x06 };

            // Set to 8 dummy cycles to run at 84 MHz
            byte[] dummy_cycles_84 = new byte[6];
            dummy_cycles_84[0] = 0x72;
            dummy_cycles_84[1] = 0x00;
            dummy_cycles_84[2] = 0x00;
            dummy_cycles_84[3] = 0x03;
            dummy_cycles_84[4] = 0x00;
            dummy_cycles_84[5] = 0x05;

            // Switch to DOPI
            byte[] switch_to_ddr_opi = new byte[6];
            switch_to_ddr_opi[0] = 0x72;
            switch_to_ddr_opi[1] = 0x00;
            switch_to_ddr_opi[2] = 0x00;
            switch_to_ddr_opi[3] = 0x00;
            switch_to_ddr_opi[4] = 0x00;
            switch_to_ddr_opi[5] = 0x02;


            TransmitTransaction(wakeup);
            TransmitTransaction(wren);
            TransmitTransaction(dummy_cycles_84);
            TransmitTransaction(wren);
            TransmitTransaction(switch_to_ddr_opi);

            spiIsInOpiMode = true;
        }

        private void TransmitTransaction(byte[] sequence) {
            foreach (var b in sequence) {
                RegisteredPeripheral.Transmit(b);
            }
            RegisteredPeripheral.FinishTransmission();
        }

        public long Size { get { return 4096; } }
        public GPIO IRQ { get; private set; }

        private readonly Queue<byte> rxQueue;
        private readonly Queue<byte> txQueue;

        private readonly DoubleWordRegisterCollection registersCollection;
        private uint dataWords;
        private uint dummyCycles;
        private byte cmdCode;
        bool spiIsInOpiMode = false;
        private readonly IValueRegisterField cmdArg;
        private bool hasArg;
        private bool lockReads;

        private enum Registers
        {
            Config = 0x00,
            DelayConfig = 0x04,
            DelayStatus = 0x08,
            Command = 0x0c,
            CmdArg = 0x10,
            CmdRbkData = 0x14,
            Status = 0x18,
            WData = 0x1c,
            EvStatus = 0x20,
            EvPending = 0x24,
            EvEnable = 0x28,
            EccAddress = 0x2c,
            EccStatus = 0x30,
        }
    }
}
