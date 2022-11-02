//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2021 Precursor Project
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System.Collections.Generic;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;

// This project is a reimplementation of the OpenCoresI2C module.

namespace Antmicro.Renode.Peripherals.I2C.Betrusted
{
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord)]
    public class BetrustedSocI2C : SimpleContainer<II2CPeripheral>, IDoubleWordPeripheral, IKnownSize
    {
        public BetrustedSocI2C(Machine machine) : base(machine)
        {
            dataToSlave = new Queue<byte>();
            dataFromSlave = new Queue<byte>();
            IRQ = new GPIO();

            irqTimeoutCallback = new ClockEntry((ulong)5, 1000, this.FinishTransaction, machine, "Irq Scheduler");

            var registersMap = new Dictionary<long, DoubleWordRegister>()
            {
                {(long)Registers.ClockPrescale, new DoubleWordRegister(this)
                    .WithValueField(0, 16, FieldMode.Read | FieldMode.Write)
                },

                {(long)Registers.Control, new DoubleWordRegister(this)
                    .WithFlag(7, out enabled)
                    .WithTag("Interrupt enable", 6, 1)
                    .WithReservedBits(0, 6)
                },

                {(long)Registers.Transmit, new DoubleWordRegister(this)
                    .WithValueField(0, 8, out transmitBuffer, FieldMode.Write)
                },
                {(long)Registers.Reset, new DoubleWordRegister(this)
                    .WithFlag(0, writeCallback: (_, val) => {
                        if (val) {
                            dataToSlave.Clear();
                            dataFromSlave.Clear();
                            UpdateInterrupts();
                        }
                    })
                },

                {(long)Registers.Receive, new DoubleWordRegister(this)
                    .WithValueField(0, 8, out receiveBuffer, FieldMode.Read)
                },

                {(long)Registers.Status, new DoubleWordRegister(this)
                    .WithFlag(7, out receivedAckFromSlaveNegated, FieldMode.Read)
                    .WithFlag(6, FieldMode.Read, valueProviderCallback: _ => false, name: "Busy")
                    .WithFlag(5, FieldMode.Read, valueProviderCallback: _ => false, name: "Arbitration lost")
                    .WithReservedBits(2, 3)
                    .WithFlag(1, FieldMode.Read, valueProviderCallback: _ => false, name: "Transfer in progress")
                    .WithFlag(0, out i2cIntIrqStatus, FieldMode.Read)
                },

                {(long)Registers.EventStatus, new DoubleWordRegister(this)
                    .WithFlag(1, FieldMode.Read, name: "TX_RX_DONE_STATUS", valueProviderCallback: _ => txRxDoneIrqStatus)
                    .WithFlag(0, FieldMode.Read, name: "I2C_INT_STATUS", valueProviderCallback: _ => i2cIntIrqStatus.Value)
                },

                {(long)Registers.EventPending, new DoubleWordRegister(this)
                    .WithFlag(1, out txRxDoneIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "TX_RX_DONE_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(0, out i2cIntIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "I2C_INT_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                },

                {(long)Registers.EventEnable, new DoubleWordRegister(this)
                    .WithFlag(1, out txRxDoneIrqEnabled, name: "TX_RX_DONE_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(0, out i2cIntIrqEnabled, name: "I2C_INT_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                },

                {(long)Registers.Command, new DoubleWordRegister(this)
                    .WithFlag(7, out generateStartCondition, FieldMode.Write)
                    .WithFlag(6, out generateStopCondition, FieldMode.Write)
                    .WithFlag(5, out readFromSlave, FieldMode.Write)
                    .WithFlag(4, out writeToSlave, FieldMode.Write)
                    .WithFlag(3, FieldMode.Read, name: "ACK", valueProviderCallback: _ => true)
                    // .WithTag("ACK", 3, 1)
                    .WithReservedBits(1, 2)
                    .WithFlag(0, FieldMode.Write, writeCallback: (_, __) => i2cIntIrqStatus.Value = false)
                    .WithWriteCallback((_, __) =>
                    {
                        if(!enabled.Value)
                        {
                            return;
                        }

                        if(generateStartCondition.Value)
                        {
                            generateStartCondition.Value = false;
                            if(transactionInProgress)
                            {
                                // repeated start - finish previous transaction first
                                SendDataToSlave();
                            }
                            else
                            {
                                transactionInProgress = true;
                            }

                            dataFromSlave.Clear();
                            if (selectedSlave != null) {
                                selectedSlave.FinishTransmission();
                            }

                            if(!TryResolveSelectedSlave(out selectedSlave))
                            {
                                return;
                            }
                        }

                        if(writeToSlave.Value)
                        {
                            writeToSlave.Value = false;
                            HandleWriteToSlaveCommand();
                        }

                        if(readFromSlave.Value)
                        {
                            readFromSlave.Value = false;
                            HandleReadFromSlaveCommand();
                        }

                        if (transactionInProgress) {
                            machine.ClockSource.AddClockEntry(irqTimeoutCallback);
                        }

                        if(generateStopCondition.Value)
                        {
                            selectedSlave.FinishTransmission();

                            generateStopCondition.Value = false;
                            if(!transactionInProgress)
                            {
                                return;
                            }

                            SendDataToSlave();
                            transactionInProgress = false;
                        }

                        shouldSendTxRxIrq = true;
                    })
                }
            };

            registers = new DoubleWordRegisterCollection(this, registersMap);
            UpdateInterrupts();
        }

        private void FinishTransaction()
        {
            machine.ClockSource.TryRemoveClockEntry(FinishTransaction);
            if (shouldSendTxRxIrq)
            {
                shouldSendTxRxIrq = false;
                txRxDoneIrqStatus = true;
                UpdateInterrupts();
                txRxDoneIrqStatus = false;
            }
        }

        public override void Reset()
        {
            registers.Reset();
            dataToSlave.Clear();
            dataFromSlave.Clear();
            UpdateInterrupts();
        }

        public uint ReadDoubleWord(long offset)
        {
            return registers.Read(offset);
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            registers.Write(offset, value);
        }

        private void UpdateInterrupts()
        {
            if (i2cIntIrqStatus.Value && i2cIntIrqEnabled.Value)
            {
                i2cIntIrqPending.Value = true;
            }
            if (txRxDoneIrqStatus && txRxDoneIrqEnabled.Value)
            {
                txRxDoneIrqPending.Value = true;
            }
            // this.Log(LogLevel.Noisy, "    Setting status: {0}, enabled: {1}, pending: {2}", txRxDoneIrqStatus, txRxDoneIrqEnabled.Value, txRxDoneIrqPending.Value);
            IRQ.Set(i2cIntIrqPending.Value || txRxDoneIrqPending.Value);
        }

        public GPIO IRQ
        {
            get;
            private set;
        }

        public long Size { get { return 0x1000; } }

        private bool TryResolveSelectedSlave(out II2CPeripheral selectedSlave)
        {
            var slaveAddress = (byte)(transmitBuffer.Value >> 1);
            if (!ChildCollection.TryGetValue(slaveAddress, out selectedSlave))
            {
                this.Log(LogLevel.Warning, "Addressing unregistered slave: 0x{0:X}", slaveAddress);
                receivedAckFromSlaveNegated.Value = true;
                return false;
            }

            receivedAckFromSlaveNegated.Value = false;
            return true;
        }

        private void HandleReadFromSlaveCommand()
        {
            if (dataFromSlave.Count == 0)
            {
                foreach (var b in selectedSlave.Read())
                {
                    dataFromSlave.Enqueue(b);
                }

                if (dataFromSlave.Count == 0)
                {
                    this.Log(LogLevel.Warning, "Trying to read from slave, but no data is available");
                    receiveBuffer.Value = 0;
                    return;
                }
            }

            receiveBuffer.Value = dataFromSlave.Dequeue();
            UpdateInterrupts();
        }

        private void SendDataToSlave()
        {
            if (dataToSlave.Count == 0 || selectedSlave == null)
            {
                this.Log(LogLevel.Warning, "Trying to send data to slave, but either no data is available or the slave is not selected");
                return;
            }

            selectedSlave.Write(dataToSlave.ToArray());
            dataToSlave.Clear();
        }

        private void HandleWriteToSlaveCommand()
        {
            if (!transactionInProgress)
            {
                this.Log(LogLevel.Warning, "Writing to slave without generating START signal");
                return;
            }

            dataToSlave.Enqueue((byte)transmitBuffer.Value);
            i2cIntIrqStatus.Value = true;
            UpdateInterrupts();
        }

        private bool transactionInProgress;
        private II2CPeripheral selectedSlave;

        private readonly Queue<byte> dataToSlave;
        private readonly Queue<byte> dataFromSlave;
        private readonly IValueRegisterField receiveBuffer;
        private readonly IValueRegisterField transmitBuffer;
        private readonly IFlagRegisterField readFromSlave;
        private readonly IFlagRegisterField writeToSlave;
        private readonly IFlagRegisterField i2cIntIrqStatus;
        private IFlagRegisterField i2cIntIrqEnabled;
        private IFlagRegisterField i2cIntIrqPending;
        private bool txRxDoneIrqStatus;
        private bool shouldSendTxRxIrq;
        private IFlagRegisterField txRxDoneIrqEnabled;
        private IFlagRegisterField txRxDoneIrqPending;
        private readonly IFlagRegisterField enabled;
        private readonly IFlagRegisterField receivedAckFromSlaveNegated;
        private readonly IFlagRegisterField generateStartCondition;
        private readonly IFlagRegisterField generateStopCondition;
        private readonly DoubleWordRegisterCollection registers;
        private ClockEntry irqTimeoutCallback;

        private enum Registers
        {
            ClockPrescale = 0x0,
            Control = 0x4,
            Transmit = 0x8,
            Receive = 0xC,
            Command = 0x10,
            Status = 0x14,
            Reset = 0x18,
            EventStatus = 0x1c,
            EventPending = 0x20,
            EventEnable = 0x24
        }
    }
}
