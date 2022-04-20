//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2021 Precursor Project
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System.Collections.Generic;
using System;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Time;
using System.Threading;

// This project is a reimplementation of the OpenCoresI2C module.

namespace Antmicro.Renode.Peripherals.I2C
{
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord)]
    public class BetrustedEcI2C : SimpleContainer<II2CPeripheral>, IDoubleWordPeripheral, IKnownSize
    {
        public BetrustedEcI2C(Machine machine) : base(machine)
        {
            dataToSlave = new Queue<byte>();
            dataFromSlave = new Queue<byte>();
            IRQ = new GPIO();

            irqTimeoutCallback = new ClockEntry((ulong)5, 1000, this.FinishTransaction, machine, "Irq Scheduler");

            var registersMap = new Dictionary<long, DoubleWordRegister>()
            {
                {(long)Registers.Prescale, new DoubleWordRegister(this)
                    .WithValueField(0, 16, FieldMode.Read | FieldMode.Write)
                },

                {(long)Registers.Control, new DoubleWordRegister(this)
                    .WithFlag(7, out enabled)
                    .WithTag("Interrupt enable", 6, 1)
                    .WithReservedBits(0, 6)
                },

                {(long)Registers.Txr, new DoubleWordRegister(this)
                    .WithValueField(0, 8, out transmitBuffer, FieldMode.Write)
                },

                {(long)Registers.Rxr, new DoubleWordRegister(this)
                    .WithValueField(0, 8, out receiveBuffer, FieldMode.Read)
                },

                {(long)Registers.Status, new DoubleWordRegister(this)
                    .WithFlag(7, out receivedAckFromSlaveNegated, FieldMode.Read)
                    .WithFlag(6, FieldMode.Read, valueProviderCallback: _ => false, name: "Busy")
                    .WithFlag(5, FieldMode.Read, valueProviderCallback: _ => false, name: "Arbitration lost")
                    .WithReservedBits(2, 3)
                    .WithFlag(1, name: "Transfer in progress", valueProviderCallback: _ => {
                        fakeTip = !fakeTip;
                        return fakeTip;
                    })
                    .WithFlag(0, out i2cIrqStatus, FieldMode.Read)
                },

                {(long)Registers.Command, new DoubleWordRegister(this)
                    .WithFlag(7, out generateStartCondition, FieldMode.Write)
                    .WithFlag(6, out generateStopCondition, FieldMode.Write)
                    .WithFlag(5, out readFromSlave, FieldMode.Write)
                    .WithFlag(4, out writeToSlave, FieldMode.Write)
                    .WithFlag(3, FieldMode.Read, name: "ACK", valueProviderCallback: _ => true)
                    .WithReservedBits(1, 2)
                    .WithFlag(0, FieldMode.Write, writeCallback: (_, __) => i2cIrqStatus.Value = false)
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

                        if (transactionInProgress && 0 == Interlocked.CompareExchange(ref irqTimeoutCallbackQueued, 1, 0)) {
                            // this.Log(LogLevel.Error, "I2C: Adding clock entry for {0}",this.irqTimeoutCallback);
                            try {
                                machine.ClockSource.AddClockEntry(irqTimeoutCallback);
                            } catch (ArgumentException ex) {
                                this.Log(LogLevel.Error, "Unable to add clock entry for timeout (queued: {0}): {1}", ex, irqTimeoutCallbackQueued);
                                // this.Log(LogLevel.Error, "I2C: Done adding clock entry for {0}",this.irqTimeoutCallback);
                            }
                        }

                        if(generateStopCondition.Value)
                        {
                            if (selectedSlave != null) {
                                selectedSlave.FinishTransmission();
                            }

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
                },
                {(long)Registers.EventStatus, new DoubleWordRegister(this)
                    .WithFlag(3, FieldMode.Read, name: "USBCC_INT_STATUS", valueProviderCallback: _ => usbccIrqStatus)
                    .WithFlag(2, FieldMode.Read, name: "GYRO_INT_STATUS", valueProviderCallback: _ => gyroIrqStatus)
                    .WithFlag(1, FieldMode.Read, name: "GG_INT_STATUS", valueProviderCallback: _ => ggIrqStatus)
                    .WithFlag(0, FieldMode.Read, name: "I2C_INT_STATUS", valueProviderCallback: _ => i2cIrqStatus.Value)
                },

                {(long)Registers.EventPending, new DoubleWordRegister(this)
                    .WithFlag(3, out usbccIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "USBCC_INT_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(2, out gyroIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "GYRO_INT_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(1, out ggIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "GG_INT_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(0, out i2cIrqPending, FieldMode.Read | FieldMode.WriteOneToClear, name: "I2C_INT_PENDING", changeCallback: (_, __) => UpdateInterrupts())
                },

                {(long)Registers.EventEnable, new DoubleWordRegister(this)
                    .WithFlag(3, out usbccIrqEnabled, name: "USBCC_INT_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(2, out gyroIrqEnabled, name: "GYRO_INT_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(1, out ggIrqEnabled, name: "GG_INT_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                    .WithFlag(0, out i2cIrqEnabled, name: "I2C_INT_ENABLE", changeCallback: (_, __) => UpdateInterrupts())
                }
            };

            registers = new DoubleWordRegisterCollection(this, registersMap);
            UpdateInterrupts();
        }

        private void FinishTransaction()
        {
            // this.Log(LogLevel.Error, "I2C: Removing clock entry for {0}",this.irqTimeoutCallback);
            machine.ClockSource.RemoveClockEntry(FinishTransaction);
            irqTimeoutCallbackQueued = 0;
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
            if (i2cIrqStatus.Value)
            {
                i2cIrqPending.Value = true;
            }
            if (ggIrqStatus)
            {
                ggIrqPending.Value = true;
            }
            if (gyroIrqStatus)
            {
                gyroIrqPending.Value = true;
            }
            if (usbccIrqStatus)
            {
                usbccIrqPending.Value = true;
            }
            // this.Log(LogLevel.Noisy, "    Setting status: {0}, enabled: {1}, pending: {2}", txRxDoneIrqStatus, txRxDoneIrqEnabled.Value, txRxDoneIrqPending.Value);
            IRQ.Set((i2cIrqPending.Value && i2cIrqEnabled.Value)
                || (ggIrqPending.Value && ggIrqEnabled.Value)
                || (gyroIrqPending.Value && gyroIrqEnabled.Value)
                || (usbccIrqPending.Value && usbccIrqEnabled.Value)
            );
        }

        public GPIO IRQ
        {
            get;
            private set;
        }

        public long Size { get { return 0x800; } }

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
            if (dataFromSlave == null) {
                this.Log(LogLevel.Error, "dataFromSlave is NULL!");
                return;
            }
            if (selectedSlave == null) {
                this.Log(LogLevel.Error, "selectedSlave is NULL!");
                return;
            }
            // if (dataFromSlave.Count == 0)
            // {
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
            // }

            if (receiveBuffer == null) {
                this.Log(LogLevel.Error, "receiveBuffer is NULL!");
                dataFromSlave.Dequeue();
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
            i2cIrqStatus.Value = true;
            UpdateInterrupts();
        }

        private bool transactionInProgress;
        // Just toggle TIP here to fake it.
        private bool fakeTip;
        private II2CPeripheral selectedSlave;

        private readonly Queue<byte> dataToSlave;
        private readonly Queue<byte> dataFromSlave;
        private readonly IValueRegisterField receiveBuffer;
        private readonly IValueRegisterField transmitBuffer;
        private readonly IFlagRegisterField readFromSlave;
        private readonly IFlagRegisterField writeToSlave;

        private IFlagRegisterField i2cIrqEnabled;
        private IFlagRegisterField ggIrqEnabled;
        private IFlagRegisterField gyroIrqEnabled;
        private IFlagRegisterField usbccIrqEnabled;
        private IFlagRegisterField i2cIrqStatus;
        private bool ggIrqStatus;
        private bool gyroIrqStatus;
        private bool usbccIrqStatus;
        private IFlagRegisterField i2cIrqPending;
        private IFlagRegisterField ggIrqPending;
        private IFlagRegisterField gyroIrqPending;
        private IFlagRegisterField usbccIrqPending;

        private bool shouldSendTxRxIrq;
        private bool txRxDoneIrqStatus;
        private long irqTimeoutCallbackQueued;
        private readonly IFlagRegisterField enabled;
        private readonly IFlagRegisterField receivedAckFromSlaveNegated;
        private readonly IFlagRegisterField generateStartCondition;
        private readonly IFlagRegisterField generateStopCondition;
        private readonly DoubleWordRegisterCollection registers;
        private ClockEntry irqTimeoutCallback;

        private enum Registers
        {
            Prescale = 0x0,
            Control = 0x4,
            Txr = 0x8,
            Rxr = 0xC,
            Command = 0x10,
            Status = 0x14,
            BitbangMode = 0x18,
            Bb = 0x1c,
            BbR = 0x20,
            EventStatus = 0x24,
            EventPending = 0x28,
            EventEnable = 0x2c
        }
    }
}
