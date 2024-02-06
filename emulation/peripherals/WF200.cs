//
// Copyright (c) 2010-2018 Antmicro
// Copyright (c) 2011-2015 Realtime Embedded
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Peripherals.SPI;
using Antmicro.Renode.Time;
using System.Collections.Generic;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Network;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.Network.Betrusted
{
    public class WF200 : ISPIPeripheral, IMACInterface, IProvidesRegisterCollection<DoubleWordRegisterCollection>
    {
        public WF200()
        {
            MAC = EmulationManager.Instance.CurrentEmulation.MACRepository.GenerateUniqueMAC();
            request = new byte[10240];
            response = new byte[10240];
            pendingTxMessages = new Queue<WfxMessage>();
            IRQ = new GPIO();
            RegistersCollection = new DoubleWordRegisterCollection(this);

            Registers.Config.Define32(this)
                .WithFlag(0, FieldMode.Read | FieldMode.Write, name: "csn_framing_error")
                .WithFlag(1, FieldMode.Read | FieldMode.Write, name: "hif_underrun")
                .WithFlag(2, FieldMode.Read | FieldMode.Write, name: "msg_underrun")
                .WithFlag(3, FieldMode.Read | FieldMode.Write, name: "hif_no_queue")
                .WithFlag(4, FieldMode.Read | FieldMode.Write, name: "hif_overrun")
                .WithFlag(5, FieldMode.Read | FieldMode.Write, name: "hif_overrun_2")
                .WithFlag(6, FieldMode.Read | FieldMode.Write, name: "hif_no_input_queue")
                .WithFlag(7, FieldMode.Read | FieldMode.Write, name: "spi_cs_framing_disable")
                .WithValueField(8, 2, out ByteOrder, FieldMode.Read | FieldMode.Write, name: "byte_order")
                .WithFlag(10, out DirectAccessMode, FieldMode.Read | FieldMode.Write, changeCallback: (_, newMode) =>
                    {
                        // When we change from "direct mode" to "queue mode", queue this custom packet
                        if (!newMode)
                        {
                            configurationOffset = 0x08;
                            this.Log(LogLevel.Noisy, "Moving to Queue mode. Queueing initial startup message.");
                            byte[] initial_message = new byte[198];
                            initial_message[0] = (byte)(initial_message.Length - 2);
                            initial_message[1] = 0x00;
                            initial_message[2] = (byte)WfxMessage.Indications.Startup;
                            initial_message[3] = 0x00;

                            // uint32_t status
                            initial_message[4] = 0x00;
                            initial_message[5] = 0x00;
                            initial_message[6] = 0x00;
                            initial_message[7] = 0x00;

                            // uint16_t hardware_id
                            initial_message[8] = 0x03;
                            initial_message[9] = 0x10;

                            // uint8_t opn[14]
                            initial_message[10] = 0x57;
                            initial_message[11] = 0x46;
                            initial_message[12] = 0x32;
                            initial_message[13] = 0x30;
                            initial_message[14] = 0x30;
                            initial_message[15] = 0x44;
                            initial_message[16] = 0x00;
                            initial_message[17] = 0x00;
                            initial_message[18] = 0x00;
                            initial_message[19] = 0x00;
                            initial_message[20] = 0x00;
                            initial_message[21] = 0x00;
                            initial_message[22] = 0x00;
                            initial_message[23] = 0x00;

                            // uint8_t uid[8]
                            initial_message[24] = 0xC0;
                            initial_message[25] = 0x02;
                            initial_message[26] = 0x11;
                            initial_message[27] = 0x1C;
                            initial_message[28] = 0x00;
                            initial_message[29] = 0x76;
                            initial_message[30] = 0x4A;
                            initial_message[31] = 0x05;

                            // uint16_t num_inp_ch_bufs
                            initial_message[32] = 0x1E;
                            initial_message[33] = 0x00;

                            // uint16_t size_inp_ch_buf
                            initial_message[34] = 0x50;
                            initial_message[35] = 0x06;

                            // uint8_t num_links_aP
                            initial_message[36] = 0x08;

                            // uint8_t num_interfaces
                            initial_message[37] = 0x02;

                            // uint8_t mac_addrs[0][6]
                            initial_message[38] = MAC.Bytes[0];
                            initial_message[39] = MAC.Bytes[1];
                            initial_message[40] = MAC.Bytes[2];
                            initial_message[41] = MAC.Bytes[3];
                            initial_message[42] = MAC.Bytes[4];
                            initial_message[43] = MAC.Bytes[5];

                            // uint8_t mac_addrs[1][6]
                            initial_message[44] = 0x84;
                            initial_message[45] = 0xFD;
                            initial_message[46] = 0x27;
                            initial_message[47] = 0x18;
                            initial_message[48] = 0xE7;
                            initial_message[49] = 0x47;

                            // uint8_t api_version_major
                            initial_message[50] = 0x07;

                            // uint8_t api_version_minor
                            initial_message[51] = 0x03;

                            // sl_wfx_capabilities_s
                            initial_message[52] = 0x00;
                            initial_message[53] = 0x00;
                            initial_message[54] = 0x00;
                            initial_message[55] = 0x00;

                            // firmware_build
                            initial_message[56] = 0x03;

                            // firmware_minor
                            initial_message[57] = 0x0C;

                            // firmware_major
                            initial_message[58] = 0x03;

                            // firmware_type
                            initial_message[59] = 0x01;

                            // uint8_t disabled_channel_list[2]
                            initial_message[60] = 0x00;
                            initial_message[61] = 0x00;

                            // uint8_t regul_sel_mode_info
                            initial_message[62] = 0xC0;

                            // uint8_t otp_phy_info
                            initial_message[63] = 0x00;

                            // uint32_t supported_rate_mask
                            initial_message[64] = 0xCF;
                            initial_message[65] = 0xFF;
                            initial_message[66] = 0x3F;
                            initial_message[67] = 0x00;

                            // uint8_t firmware_label[128]
                            initial_message[68] = 0x57;
                            initial_message[69] = 0x46;
                            initial_message[70] = 0x32;
                            initial_message[71] = 0x30;
                            initial_message[72] = 0x30;
                            initial_message[73] = 0x5F;
                            initial_message[74] = 0x41;
                            initial_message[75] = 0x53;
                            initial_message[76] = 0x49;
                            initial_message[77] = 0x43;
                            initial_message[78] = 0x5F;
                            initial_message[79] = 0x57;
                            initial_message[80] = 0x46;
                            initial_message[81] = 0x4D;
                            initial_message[82] = 0x5F;
                            initial_message[83] = 0x28;
                            initial_message[84] = 0x4A;
                            initial_message[85] = 0x65;
                            initial_message[86] = 0x6E;
                            initial_message[87] = 0x6B;
                            initial_message[88] = 0x69;
                            initial_message[89] = 0x6E;
                            initial_message[90] = 0x73;
                            initial_message[91] = 0x29;
                            initial_message[92] = 0x5F;
                            initial_message[93] = 0x46;
                            initial_message[94] = 0x57;
                            initial_message[95] = 0x33;
                            initial_message[96] = 0x2E;
                            initial_message[97] = 0x31;
                            initial_message[98] = 0x32;
                            initial_message[99] = 0x2E;
                            initial_message[100] = 0x33;

                            // Return a faked version of the CONTROL register (the reference code calls this `piggy-backed`)
                            initial_message[196] = 0x00;
                            initial_message[197] = 0x30;
                            pendingTxMessages.Enqueue(new WfxMessage(WfxMessage.IdType.Indication, initial_message));
                        }
                    }, name: "direct_access_mode")
                .WithReservedBits(11, 1)
                .WithFlag(12, out CpuClockDisable, FieldMode.Read | FieldMode.Write, name: "cpu_clk_disable")
                // Always return `false` here to fake memory being read back immediately
                .WithFlag(13, FieldMode.Read | FieldMode.Write, valueProviderCallback: _ => false, name: "direct_pre_fetch_shared_ram")
                .WithFlag(14, out CpuReset, FieldMode.Read | FieldMode.Write, name: "cpu_reset")
                .WithFlag(15, FieldMode.Read | FieldMode.Write, name: "disable_dat1_mode")
                .WithValueField(16, 2, FieldMode.Read | FieldMode.Write, name: "dout_posedge_ena")
                .WithFlag(18, FieldMode.Read | FieldMode.Write, name: "disable_dat1_irq")
                .WithFlag(19, FieldMode.Read | FieldMode.Write, name: "sdio_disable_crc_check")
                .WithReservedBits(20, 4)
                .WithValueField(24, 8, valueProviderCallback: _ => 1, name: "device id")
            ;

            Registers.Control.Define32(this)
                .WithValueField(0, 12, FieldMode.Read, valueProviderCallback: _ =>
                {
                    if (pendingTxMessages.Count == 0)
                    {
                        return 0;
                    }
                    // Return the number of 16-bit words, not including the header
                    return (uint)pendingTxMessages.Peek().Data.Count / 2 - 1;
                }, name: "next_output_queue_item_length")
                .WithFlag(12, out WlanReady, FieldMode.Write | FieldMode.Read, name: "wlan_wup")
                .WithFlag(13, FieldMode.Read, valueProviderCallback: _ => WlanReady.Value, name: "wlan_rdy")
                .WithValueField(14, 2, FieldMode.Read, valueProviderCallback: _ =>
                {
                    if (pendingTxMessages.Count == 0)
                    {
                        return 0;
                    }
                    return pendingTxMessages.Peek().Id;
                }, name: "frame_type_info")
                .WithReservedBits(16, 16)
            ;

            Registers.MemoryAddress.Define32(this)
                .WithValueField(0, 32, out MemoryAddress, name: "memory_address")
            ;

            Reset();
        }

        public GPIO IRQ { get; private set; }

        public void Reset()
        {
            mode = Mode.ReadingHeader1;
            ByteOrder.Value = 0;
            WlanReady.Value = false;
            CpuClockDisable.Value = true;
            DirectAccessMode.Value = true;
            CpuReset.Value = true;
            spiHeader = 0;
            pendingTxMessages.Clear();
            configurationOffset = 0;
        }

        public MACAddress MAC { get; set; }

        public void ReceiveFrame(EthernetFrame frame)
        {
            if (!frame.DestinationMAC.IsBroadcast && frame.DestinationMAC != MAC)
            {
                return;
            }
            // this.Log(LogLevel.Info, "Received frame {0}.", frame);

            // sl_wfx_received_ind_body_t
            // Strip off the Ethernet footer
            var frame_length = frame.Bytes.Length - 4;
            var frame_padding = (4 - (frame_length & 3)) & 3;

            var frame_bytes = new byte[frame_length + 6 + frame_padding];

            // uint8_t frame_type
            frame_bytes[0] = 0;

            // uint8_t frame_padding
            frame_bytes[1] = (byte)frame_padding;

            // uint16_t frame_length
            frame_bytes[2] = (byte)frame_length;
            frame_bytes[3] = (byte)(frame_length >> 8);

            // uint8_t padding[frame_padding] -- may be 0, 1, 2, or 3

            // uint8_t frame[frame_length]
            Array.Copy(frame.Bytes, 0, frame_bytes, 4 + frame_padding, frame_length);

            // Faked CONTROL register
            frame_bytes[frame_bytes.Length - 1] = 0x30;

            var receivedPacketMessage = new WfxMessage(WfxMessage.Ethernet.Received, frame_bytes);
            pendingTxMessages.Enqueue(receivedPacketMessage);
        }

        public byte Transmit(byte data)
        {
            if (DirectAccessMode.Value)
            {
                return HandleDirectAccessMode(data);
            }
            else
            {
                return HandleDirectAccessMode(data);
            }
        }

        private byte HandleDirectAccessMode(byte data)
        {
            if (spiLength == 0 && (mode != Mode.ReadingHeader1) && (mode != Mode.ReadingHeader2))
            {
                this.Log(LogLevel.Error, "Spi had {0} bytes left yet was still in mode {1}", spiLength, mode);
                mode = Mode.ReadingHeader1;
            }

            switch (mode)
            {
                case Mode.ReadingHeader1:
                    spiHeader = ((uint)data) << 8;
                    mode = Mode.ReadingHeader2;
                    return 0x00;
                case Mode.ReadingHeader2:
                    spiHeader |= ((uint)data) << 0;
                    // this.Log(LogLevel.Noisy, "SPI header was {0:X4}", spiHeader);
                    spiLength = BitHelper.GetValue(spiHeader, 0, 12) * 2;
                    counter = 0;
                    spiIsRead = BitHelper.IsBitSet(spiHeader, 15);
                    var addr = BitHelper.GetValue(spiHeader, 12, 3);
                    if (addr == 7)
                    {
                        this.Log(LogLevel.Error, "Invalid register address 7");
                        mode = Mode.Error;
                        return 0xff;
                    }
                    spiAddress = (Registers)addr;

                    // this.Log(LogLevel.Noisy, "Detected a {0} on register {1} of {2} bytes, mode {3}", spiIsRead ? "Read" : "Write", spiAddress, spiLength, ByteOrder.Value);
                    if (spiIsRead)
                    {
                        PrepareReadRegister();
                        mode = Mode.ReadingRegister;
                    }
                    else
                    {
                        mode = Mode.WritingRegister;
                    }
                    return 0x00;
                case Mode.ReadingRegister:
                    var ret = response[counter];
                    counter += 1;
                    spiLength -= 1;
                    if (spiLength == 0)
                    {
                        mode = Mode.ReadingHeader1;
                    }
                    return ret;
                case Mode.WritingRegister:
                    // this.Log(LogLevel.Noisy, "Appending {0:X2} to request buffer @ {1}", data, counter);
                    request[counter] = data;
                    counter += 1;
                    spiLength -= 1;
                    if (spiLength == 0)
                    {
                        mode = Mode.ReadingHeader1;
                        HandleWriteRegister();
                    }
                    break;
                case Mode.Error:
                    counter += 1;
                    spiLength -= 1;
                    if (spiLength == 0)
                    {
                        mode = Mode.ReadingHeader1;
                    }
                    return 0xff;
                default:
                    this.Log(LogLevel.Error, "Unexpected mode");
                    break;
            }
            return 0x00;
        }

        private void PrepareReadRegister()
        {
            switch (spiAddress)
            {
                case Registers.Config:
                case Registers.Control:
                case Registers.MemoryAddress:
                    {
                        var val = RegistersCollection.Read((long)spiAddress);
                        Array.Clear(response, 0, response.Length);
                        if ((ByteOrder.Value == 0) || (spiAddress == Registers.Config))
                        {
                            if ((spiLength == 4) || (spiLength == 2))
                            {
                                response[0] = (byte)(val >> 8);
                                response[1] = (byte)(val >> 0);
                                response[2] = (byte)(val >> 24);
                                response[3] = (byte)(val >> 16);
                            }
                            else
                            {
                                this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                            }
                        }
                        else if (ByteOrder.Value == 1)
                        {
                            if (spiLength == 4)
                            {
                                response[0] = (byte)(val >> 24);
                                response[1] = (byte)(val >> 16);
                                response[2] = (byte)(val >> 8);
                                response[3] = (byte)(val >> 0);
                            }
                            else
                            {
                                this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                            }
                        }
                        else if (ByteOrder.Value == 2)
                        {
                            if ((spiLength == 2) | ((spiLength == 4) && (spiAddress == Registers.Control)))
                            {
                                response[1] = (byte)(val >> 8);
                                response[0] = (byte)(val >> 0);
                            }
                            else if (spiLength == 4)
                            {
                                response[0] = (byte)(val >> 0);
                                response[1] = (byte)(val >> 8);
                                response[2] = (byte)(val >> 16);
                                response[3] = (byte)(val >> 24);
                            }
                            else
                            {
                                this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                            }
                        }
                        else
                        {
                            this.Log(LogLevel.Error, "Unrecognized byte order {0}", ByteOrder.Value);
                        }
                        // if (spiLength == 2)
                        // {
                        //     this.Log(LogLevel.Noisy, "Returning [{0:X2} {1:X2}] (originally \"{2}\" {3:X8})", response[0], response[1], spiAddress, val);
                        // }
                        // else if (spiLength == 4)
                        // {
                        //     this.Log(LogLevel.Noisy, "Returning [{0:X2} {1:X2} {2:X2} {3:X2}] (originally \"{4}\" {5:X8})", response[0], response[1], response[2], response[3], spiAddress, val);
                        // }
                    }
                    break;
                case Registers.SharedRamPort:
                    {
                        uint val = 0;
                        switch (MemoryAddress.Value)
                        {
                            case 0x09004000:
                                this.Log(LogLevel.Debug, "Faking return value for bootloader address");
                                val = 0x23abc88e;
                                break;
                            // ADDR_DWL_CTRL_AREA_GET
                            case 0x0900c008:
                                // downloadOffset += 0x8000;
                                this.Log(LogLevel.Debug, "Faking SWL_CTRL_AREA_GET to 0x{0:X08}", downloadOffset);
                                val = downloadOffset;
                                break;
                            // NCP status
                            case 0x0900c010:
                                if (ncpStateVal == 0)
                                {
                                    this.Log(LogLevel.Debug, "Faking NCP_STATE_INFO_READY");
                                    val = 0xBD53EF99;
                                    ncpStateVal = 1;
                                }
                                else if (ncpStateVal == 1)
                                {
                                    this.Log(LogLevel.Debug, "Faking NCP_STATE_READY");
                                    val = 0x87654321;
                                    ncpStateVal = 2;
                                }
                                else
                                {
                                    this.Log(LogLevel.Debug, "Faking NCP_STATE_AUTH_OK");
                                    val = 0xD4C64A99;
                                    ncpStateVal = 0;
                                }
                                break;
                            case 0x0900c0cc:
                                this.Log(LogLevel.Debug, "Faking WF200 keyset to be 0xA19C9700");
                                val = 0xA19C9700;
                                break;
                            default:
                                this.Log(LogLevel.Debug, "Unrecognized SRAM port address: {0:08X}", MemoryAddress.Value);
                                break;
                        }
                        // 0x23abc88e
                        if (ByteOrder.Value == 0)
                        {
                            response[0] = (byte)(val >> 8);
                            response[1] = (byte)(val >> 0);
                            response[2] = (byte)(val >> 24);
                            response[3] = (byte)(val >> 16);
                        }
                        else if (ByteOrder.Value == 2)
                        {
                            response[0] = (byte)(val >> 0);
                            response[1] = (byte)(val >> 8);
                            response[2] = (byte)(val >> 16);
                            response[3] = (byte)(val >> 24);
                        }
                        else
                        {
                            this.Log(LogLevel.Error, "Unrecognized byte order {0}", ByteOrder.Value);
                        }
                    }
                    break;
                case Registers.IoQueue:
                    if (pendingTxMessages.Count == 0)
                    {
                        this.Log(LogLevel.Error, "Tried to transmit a packet when none existed");
                        break;
                    }
                    var message = pendingTxMessages.Dequeue();
                    // this.Log(LogLevel.Debug, "Copying Pending Tx Message to response buffer: {0}", message);
                    message.Data.CopyTo(response);

                    // Increment the `word counter` bits -- it seems to go 0, 8, 10, 18, 20, 28, 30, 38, 0, ...
                    response[3] |= wordCounter;
                    wordCounter = (byte)((wordCounter + 0x8) & 0x38);
                    break;
            }
        }
        private uint ncpStateVal = 0;
        private byte wordCounter;

        private void HandleWriteRegister()
        {
            // this.Log(LogLevel.Noisy, "Updating register \"{0}\" with {1} bytes of data", spiAddress, counter);
            switch (spiAddress)
            {
                case Registers.Config:
                case Registers.Control:
                case Registers.MemoryAddress:
                    {
                        var existing = RegistersCollection.Read((long)spiAddress);
                        // Control register is always Mode 0
                        var val = GetWriteRegister(existing, spiAddress == Registers.Config ? 0 : ByteOrder.Value);
                        var logLevel = LogLevel.Debug;
                        if (spiAddress == Registers.MemoryAddress)
                        {
                            logLevel = LogLevel.Noisy;
                        }
                        this.Log(logLevel, "Updating value of \"{0}\" from {1:X8} -> {2:X8}", spiAddress, existing, val);
                        RegistersCollection.Write((long)spiAddress, val);

                        break;
                    }
                case Registers.GeneralPurpose:
                    {
                        var val = GetWriteRegister(0, ByteOrder.Value);
                        var gpReg = (val >> 24) & 0xff;
                        this.Log(LogLevel.Debug, "Updating value of GP reg {0} to 0x{1:X8}", gpReg, val);
                        break;
                    }
                case Registers.SharedRamPort:
                    if ((counter != 4) && (counter != 2))
                    {
                        this.Log(LogLevel.Noisy, "Ingoring non-word write");
                        break;
                    }
                    else
                    {
                        var val = GetWriteRegister(0, ByteOrder.Value);
                        switch (MemoryAddress.Value)
                        {
                            // ADDR_DWL_CTRL_AREA_GET
                            case 0x0900c008:
                            // ADDR_DWL_CTRL_AREA_PUT
                            case 0x0900c004:
                                downloadOffset = val;
                                this.Log(LogLevel.Noisy, "Setting download address to 0x{0:X08}", downloadOffset);
                                break;
                            // ADDR_DWL_CTRL_AREA_HOST_STATUS
                            case 0x0900c00c:
                                this.Log(LogLevel.Debug, "Ignoring HOST_STATUS update of {0}", val);
                                break;
                            default:
                                this.Log(LogLevel.Error, "Unrecognized RAM port address 0x{0:X08} -- dropping val 0x{1:X08}", MemoryAddress.Value, val);
                                break;
                        }
                    }
                    // The MemoryAddress is updated, particularly when doing firmware writes
                    MemoryAddress.Value += (uint)counter;
                    break;
                case Registers.IoQueue:
                    // this.Log(LogLevel.Warning, "Recieved request, spiLength: {0} request length: {1}", counter, request.Length);
                    byte[] request_data = new byte[counter];
                    Array.Copy(request, request_data, counter);
                    WfxMessage req = new WfxMessage(WfxMessage.IdType.Request, request_data);
                    // this.Log(LogLevel.Warning, "Recieved request: {0}", req);
                    HandleRequest(req);
                    break;
                default:
                    this.Log(LogLevel.Error, "Unhandled register write: \"{0}\"", spiAddress);
                    break;
            }
        }

        private void HandleRequest(WfxMessage request)
        {
            // Default response of "Okay", plus a piggy-backed read of the CONTROL register with no data
            var okayResponse = new byte[6];
            okayResponse[5] = 0x30;

            if (request.Type != WfxMessage.IdType.Request)
            {
                this.Log(LogLevel.Error, "Message was a {0}, not Request!", request.Type);
                return;
            }

            var req = (WfxMessage.Requests)request.Id;
            if (req == WfxMessage.Requests.Configuration)
            {
                var message = new WfxMessage(WfxMessage.Confirmations.Configuration, okayResponse, configurationOffset);
                configurationOffset += 0x08;
                // this.Log(LogLevel.Debug, "Sending response for configuration message: {0}", message);
                pendingTxMessages.Enqueue(message);
            }
            else if (req == WfxMessage.Requests.Connect)
            {
                var message = new WfxMessage(WfxMessage.Confirmations.Connect, okayResponse);
                // this.Log(LogLevel.Debug, "Sending response for connection request: {0}", message);
                pendingTxMessages.Enqueue(message);

                var connect_ind = new byte[18];
                // uint32_t status /* WFM_STATUS_SUCCESS */
                connect_ind[0] = 0x00;
                connect_ind[1] = 0x00;
                connect_ind[2] = 0x00;
                connect_ind[3] = 0x00;

                // uint8_t mac[6]
                connect_ind[4] = 0xF0;
                connect_ind[5] = 0x9F;
                connect_ind[6] = 0xC2;
                connect_ind[7] = 0x24;
                connect_ind[8] = 0x2E;
                connect_ind[9] = 0x45;

                // uint16_t channel
                connect_ind[10] = 0x0B;
                connect_ind[11] = 0x00;

                // uint8_t beacon_interval
                connect_ind[12] = 0x64;

                // uint8_t dtim_period
                connect_ind[13] = 0x03;

                // uint16_t max_phy_rate
                connect_ind[14] = 0x15;
                connect_ind[15] = 0x00;

                // uint16_t dummy CONTROL register
                connect_ind[16] = 0x00;
                connect_ind[17] = 0x30;
                message = new WfxMessage(WfxMessage.Indications.Connect, okayResponse);
                pendingTxMessages.Enqueue(message);

            }
            else if (req == WfxMessage.Requests.StartScan)
            {
                var machine = this.GetMachine();
                var time_domain = machine.ElapsedVirtualTime.Domain;

                var message = new WfxMessage(WfxMessage.Confirmations.StartScan, okayResponse);
                // this.Log(LogLevel.Debug, "Starting a scan and responding with {0}", message);
                pendingTxMessages.Enqueue(message);

                for (var i = 0; i < 10; i++)
                {
                    var ap1 = new byte[54];
                    // uint32_t ssid_len
                    ap1[0] = 8;

                    // uint8_t ssid[32]
                    ap1[4] = (byte)'R';
                    ap1[5] = (byte)'e';
                    ap1[6] = (byte)'n';
                    ap1[7] = (byte)'o';
                    ap1[8] = (byte)'d';
                    ap1[9] = (byte)'e';
                    ap1[10] = (byte)' ';
                    ap1[11] = (byte)(i + 48);

                    // uint8_t mac[6]
                    ap1[36] = (byte)(0x14 + i);
                    ap1[37] = (byte)(0x13 + i);
                    ap1[38] = (byte)(0x15 + i);
                    ap1[39] = (byte)(0x26 + i);
                    ap1[40] = (byte)(0x37 + i);
                    ap1[41] = (byte)(0x48 + i);

                    // uint16_t channel
                    ap1[42] = (byte)(1 + i);
                    ap1[43] = 0;

                    // uint8_t security_mode
                    ap1[44] = (1 << 2) | (1 << 6);

                    // uint8_t reserved[3]

                    // uint16_t rcpi
                    ap1[48] = (byte)(0x6e + i);
                    ap1[49] = 0x00;

                    // uint16_t ie_data_length
                    ap1[50] = 0x00;
                    ap1[51] = 0x00;

                    // // uint16_t dummy CONTROL register
                    ap1[52] = 0x00;
                    ap1[53] = 0x30;

                    var reply_message = new WfxMessage(WfxMessage.Indications.ScanResult, ap1);
                    var ts = new TimeStamp(TimeInterval.FromMilliseconds(50) + machine.ElapsedVirtualTime.TimeElapsed, time_domain);
                    this.GetMachine().HandleTimeDomainEvent(pendingTxMessages.Enqueue, reply_message, ts);
                }

                var final_reply_message = new WfxMessage(WfxMessage.Indications.ScanComplete, okayResponse);
                var final_ts = new TimeStamp(TimeInterval.FromMilliseconds(700) + machine.ElapsedVirtualTime.TimeElapsed, time_domain);
                this.GetMachine().HandleTimeDomainEvent(pendingTxMessages.Enqueue, final_reply_message, final_ts);
            }

            else if (req == WfxMessage.Requests.SendFrame)
            {
                var payload = request.Payload();
                var sendFrameResponse = new byte[10];
                // uint32_t status
                sendFrameResponse[0] = 0;
                sendFrameResponse[1] = 0;
                sendFrameResponse[2] = 0;
                sendFrameResponse[3] = 0;

                // uint16_t packet_id (from request)
                sendFrameResponse[4] = payload[2];
                sendFrameResponse[5] = payload[3];
                sendFrameResponse[6] = 0;

                // uint16_t reserved
                sendFrameResponse[7] = 0;

                // Dummy CONTROL register
                sendFrameResponse[8] = 0;
                sendFrameResponse[9] = 0x30;

                // Our bus is 16-bits, but sometimes we want to transmit an odd number of bytes.
                // Support omitting the last byte if necessary.
                var roundingError = payload[4] & 1;
                var frame_data = payload.GetRange(8, payload.Count - 8 - roundingError).ToArray();
                var packet_data_length = (payload[4] << 0) | (payload[5] << 8) | (payload[6] << 16) | (payload[7] << 24);
                if (frame_data.Length != packet_data_length)
                {
                    this.Log(LogLevel.Warning, "host reported payload length of {0}, but transmitted a payload length of {1}", packet_data_length, frame_data.Length);
                }
                if (!Misc.TryCreateFrameOrLogWarning(this, frame_data, out var frame, addCrc: true))
                {
                    return;
                }
                // this.Log(LogLevel.Noisy, "Sending frame {0}.", frame);
                FrameReady?.Invoke(frame);

                pendingTxMessages.Enqueue(new WfxMessage(WfxMessage.Confirmations.SendFrame, sendFrameResponse));
            }
            else if (req == WfxMessage.Requests.GetSignalStrength)
            {
                var response = new byte[10];
                // uint32_t status

                // uint32_t rcpi
                response[4] = 0x54;
                response[5] = 0x00;
                response[6] = 0x00;
                response[7] = 0x00;

                response[8] = 0x00;
                response[9] = 0x30;
                pendingTxMessages.Enqueue(new WfxMessage(WfxMessage.Confirmations.GetSignalStrength, response));
            }
            else if (req == WfxMessage.Requests.SetArpIpAddress)
            {
                var message = new WfxMessage(WfxMessage.Confirmations.SetArpIpAddress, okayResponse, configurationOffset);
                // this.Log(LogLevel.Info, "Sending response for configuration message: {0}", message);
                pendingTxMessages.Enqueue(message);
            }
            else if (req == WfxMessage.Requests.Disconnect)
            {
                var message = new WfxMessage(WfxMessage.Confirmations.Disconnect, okayResponse, configurationOffset);
                pendingTxMessages.Enqueue(message);
            }
            else
            {
                this.Log(LogLevel.Error, "Unhandled request: {0}", request);
            }
        }

        private uint GetWriteRegister(uint val, ulong order)
        {
            if (order == 0)
            {
                if (counter == 2)
                {
                    val = (val & ~(uint)0x0000ff00) | (((uint)request[0]) << 8);
                    val = (val & ~(uint)0x000000ff) | (((uint)request[1]) << 0);
                }
                else if (counter == 4)
                {
                    val = (val & ~(uint)0x0000ff00) | (((uint)request[0]) << 8);
                    val = (val & ~(uint)0x000000ff) | (((uint)request[1]) << 0);
                    val = (val & ~(uint)0xff000000) | (((uint)request[2]) << 24);
                    val = (val & ~(uint)0x00ff0000) | (((uint)request[3]) << 16);
                }
                else
                {
                    this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                }
            }
            else if (order == 1)
            {
                if (spiLength == 4)
                {
                    val = (val & ~(uint)0xff000000) | (((uint)request[3]) << 24);
                    val = (val & ~(uint)0x00ff0000) | (((uint)request[2]) << 16);
                    val = (val & ~(uint)0x0000ff00) | (((uint)request[1]) << 8);
                    val = (val & ~(uint)0x000000ff) | (((uint)request[0]) << 0);
                }
                else
                {
                    this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                }
            }
            else if (order == 2)
            {
                if (counter == 2)
                {
                    val = (val & ~(uint)0x000000ff) | (((uint)request[0]) << 0);
                    val = (val & ~(uint)0x0000ff00) | (((uint)request[1]) << 8);
                    // response[1] = (byte)(val >> 8);
                    // response[0] = (byte)(val >> 0);
                }
                else if (counter == 4)
                {
                    val = (val & ~(uint)0x000000ff) | (((uint)request[0]) << 0);
                    val = (val & ~(uint)0x0000ff00) | (((uint)request[1]) << 8);
                    val = (val & ~(uint)0x00ff0000) | (((uint)request[2]) << 16);
                    val = (val & ~(uint)0xff000000) | (((uint)request[3]) << 24);
                }
                else if ((spiLength == 1024) && (spiAddress == Registers.MemoryAddress))
                {
                    // this.Log(LogLevel.Noisy, "Ignoring 1024-byte write to memory address @ {0:X08} -- probably a firmware write", MemoryAddress.Value);
                }
                else
                {
                    this.Log(LogLevel.Error, "Unrecognized byte order {0} and byte count {1}", ByteOrder.Value, counter);
                }
            }
            else
            {
                this.Log(LogLevel.Error, "Unrecognized byte order {0}", ByteOrder.Value);
            }
            return val;
        }

        public void FinishTransmission()
        {
            if ((mode != Mode.ReadingHeader1) || (spiLength > 0))
            {
                this.Log(LogLevel.Error, "Finished transmission with data still in the buffer");
            }
            mode = Mode.ReadingHeader1;
        }

        public void WriteDoubleWord(long address, uint value)
        {
            RegistersCollection.Write(address, value);
        }

        public uint ReadDoubleWord(long offset)
        {
            return RegistersCollection.Read(offset);
        }

        public event Action<EthernetFrame> FrameReady;

        public DoubleWordRegisterCollection RegistersCollection { get; private set; }
        private int counter;
        private Mode mode;
        private uint spiHeader;
        private byte configurationOffset;
        private uint spiLength;
        private bool spiIsRead;
        private Registers spiAddress;
        private readonly byte[] request;
        private readonly byte[] response;
        private IValueRegisterField MemoryAddress;
        private IValueRegisterField ByteOrder;
        private IFlagRegisterField WlanReady;
        private IFlagRegisterField CpuClockDisable;
        private IFlagRegisterField DirectAccessMode;
        private IFlagRegisterField CpuReset;
        private uint downloadOffset;
        private Queue<WfxMessage> pendingTxMessages;

        private enum Mode
        {
            ReadingHeader1,
            ReadingHeader2,
            ReadingRegister,
            WritingRegister,
            Error
        }
        private enum Registers
        {
            Config = 0x00,
            Control = 0x01,
            IoQueue = 0x02,
            AhbDirectAccess = 0x03,
            MemoryAddress = 0x04,
            SharedRamPort = 0x05,
            GeneralPurpose = 0x06,
        }
        private class WfxMessage
        {
            public WfxMessage(IdType type, byte[] data)
            {
                Type = type;
                uint length = ((uint)data[0]) | (((uint)data[1]) << 8);
                Id = data[2];
                Info = data[3];
                // Construct the packet
                Data = new List<byte>(data);
            }

            public WfxMessage(Requests request, byte[] data, byte info = (byte)0)
            {
                Id = (byte)request;
                Type = IdType.Request;
                Info = info;
                Data = new List<byte>();

                // Construct the packet
                var length = data.Length + 2;
                Data.Add((byte)length);
                Data.Add((byte)(length >> 8));
                Data.Add(Id);
                Data.Add(Info);
                Data.AddRange(data);
            }

            public WfxMessage(Indications indication, byte[] data, byte info = (byte)0)
            {
                Id = (byte)indication;
                Type = IdType.Indication;
                Info = info;
                Data = new List<byte>();

                // Construct the packet
                var length = data.Length + 2;
                Data.Add((byte)length);
                Data.Add((byte)(length >> 8));
                Data.Add(Id);
                Data.Add(Info);
                Data.AddRange(data);
            }

            public WfxMessage(Confirmations confirmation, byte[] data, byte info = (byte)0)
            {
                Id = (byte)confirmation;
                Type = IdType.Confirmation;
                Info = info;
                Data = new List<byte>();

                // Construct the packet
                var length = data.Length + 2;
                Data.Add((byte)length);
                Data.Add((byte)(length >> 8));
                Data.Add(Id);
                Data.Add(Info);
                Data.AddRange(data);
            }

            public WfxMessage(Ethernet eth, byte[] data, byte info = (byte)0)
            {
                Id = (byte)eth;
                Type = IdType.Ethernet;
                Info = info;
                Data = new List<byte>();

                // Construct the packet
                var length = data.Length + 2;
                Data.Add((byte)length);
                Data.Add((byte)(length >> 8));
                Data.Add(Id);
                Data.Add(Info);
                Data.AddRange(data);
            }


            public override string ToString()
            {
                var headerString = "{";
                if (Data.Count > 0)
                {
                    headerString += String.Format("Size: {0:X2}", Data[0]);
                }
                if (Data.Count > 1)
                {
                    headerString += String.Format("{0:X2}", Data[1]);
                }
                if (Data.Count > 2)
                {
                    headerString += String.Format(" ID:{0:X2}", Data[2]);
                }
                if (Data.Count > 3)
                {
                    headerString += String.Format(" Flags:{0:X2}", Data[3]);
                }
                headerString += "}";

                var dataString = "{";
                var idx = 0;
                foreach (var b in Data)
                {
                    idx += 1;
                    if (idx <= 4)
                    {
                        continue;
                    }
                    else if (idx == 5)
                    {
                        dataString += String.Format("{0:X2}", b);
                    }
                    else
                    {
                        dataString += String.Format(", {0:X2}", b);
                    }
                }
                dataString += "}";

                switch (Type)
                {
                    case IdType.Confirmation:
                        {
                            var id = (Confirmations)Id;
                            return $"[WfxMessage<Confirmation> Id=<{id}> Bytes={Data.Count} Header={headerString} Data={dataString}]";
                        }
                    case IdType.Indication:
                        {
                            var id = (Indications)Id;
                            return $"[WfxMessage<Indication> Id=<{id}> Bytes={Data.Count} Header={headerString} Data={dataString}]";
                        }
                    case IdType.Request:
                        {
                            var id = (Requests)Id;
                            return $"[WfxMessage<Request> Id=<{id}> Bytes={Data.Count} Header={headerString} Data={dataString}]";
                        }
                    case IdType.Ethernet:
                        {
                            var id = (Ethernet)Id;
                            return $"[WfxMessage<Ethernet> Id=<{id}> Bytes={Data.Count} Header={headerString} Data={dataString}]";
                        }
                    default:
                        return $"[WfxMessage<Unknown> Id=<{Id}> Bytes={Data.Count} Header={headerString} Data={dataString}]";
                }
            }
            public List<byte> Payload()
            {
                if (Data.Count <= 4)
                {
                    return new List<byte>();
                }
                return Data.GetRange(4, Data.Count - 4);
            }

            // One of `Requests`, `Confirmations`, or `Indications`
            public readonly byte Id;
            public readonly byte Info;
            public readonly IdType Type;
            public readonly List<byte> Data;
            public enum IdType
            {
                Confirmation = 0,
                Indication = 1,
                Management = 2,
                Ethernet = 3,
                // Requests are made from the host and never by the device
                Request = 4,
            };
            public enum Requests
            {
                Configuration = 0x09,
                ControlGpio = 0x26,
                SetSecurelinkMacKey = 0x27,
                SecurelinkExchangePubKeys = 0x28,
                SecurelinkConfigure = 0x29,
                PreventRollback = 0x2a,
                PtaSettings = 0x2b,
                PtaPriority = 0x2c,
                PtaState = 0x2d,
                SetCcaConfig = 0x2e,
                ShutDown = 0x32,
                SetMacAddress = 0x42,
                Connect = 0x43,
                Disconnect = 0x44,
                StartAp = 0x45,
                UpdateAp = 0x46,
                StopAp = 0x47,
                SendFrame = 0x4A,
                StartScan = 0x4B,
                StopScan = 0x4C,
                GetSignalStrength = 0x4E,
                DisconnectApClient = 0x4F,
                SetPmMode = 0x52,
                AddMulticastAddr = 0x53,
                RemoveMulticastAddr = 0x54,
                SetMaxApClientCount = 0x55,
                SetMaxApClientInactivity = 0x56,
                SetRoamParameters = 0x57,
                SetTxRateParameters = 0x58,
                SetArpIpAddress = 0x59,
                SetNsIpAddress = 0x5A,
                SetBroadcastFilter = 0x5B,
                SetScanParameters = 0x5C,
                SetUnicastFilter = 0x5D,
                AddWhitelistAddr = 0x5E,
                AddBlacklistAddr = 0x5F,
                SetMaxTxPower = 0x60,
                GetMaxTxPower = 0x61,
                GetPmk = 0x62,
                GetApClientSignalStrength = 0x63,
                ExtAuth = 0x64,
            };
            public enum Confirmations
            {
                Configuration = 0x09,
                ControlGpio = 0x26,
                SetSecurelinkMacKey = 0x27,
                SecurelinkExchangePubKeys = 0x28,
                SecurelinkConfigure = 0x29,
                PreventRollback = 0x2a,
                PtaSettings = 0x2b,
                PtaPriority = 0x2c,
                PtaState = 0x2d,
                SetCcaConfig = 0x2e,
                SetMacAddress = 0x42,
                Connect = 0x43,
                Disconnect = 0x44,
                StartAp = 0x45,
                UpdateAp = 0x46,
                StopAp = 0x47,
                SendFrame = 0x4A,
                StartScan = 0x4B,
                StopScan = 0x4C,
                GetSignalStrength = 0x4E,
                DisconnectApClient = 0x4F,
                SetPmMode = 0x52,
                AddMulticastAddr = 0x53,
                RemoveMulticastAddr = 0x54,
                SetMaxApClientCount = 0x55,
                SetMaxApClientInactivity = 0x56,
                SetRoamParameters = 0x57,
                SetTxRateParameters = 0x58,
                SetArpIpAddress = 0x59,
                SetNsIpAddress = 0x5A,
                SetBroadcastFilter = 0x5B,
                SetScanParameters = 0x5C,
                SetUnicastFilter = 0x5D,
                AddWhitelistAddr = 0x5E,
                AddBlacklistAddr = 0x5F,
                SetMaxTxPower = 0x60,
                GetMaxTxPower = 0x61,
                GetPmk = 0x62,
                GetApClientSignalStrength = 0x63,
                ExtAuth = 0x64
            };
            public enum Indications
            {
                Connect = 0xc3,
                Disconnect = 0xc4,
                StartAp = 0xc5,
                StopAp = 0xc7,
                Received = 0xca,
                ScanResult = 0xcb,
                ScanComplete = 0xcc,
                ApClientConnected = 0xcd,
                ApClientRejected = 0xce,
                ApClientDisconnected = 0xcf,
                ExtAuth = 0xd2,
                PsModeError = 0xd3,
                Exception = 0xe0,
                Startup = 0xe1,
                Wakeup = 0xe2,
                Generic = 0xe3,
                Error = 0xe4,
                SecurelinkExchagePubKeys = 0xe5,
            };
            public enum Ethernet
            {
                Received = 0xca,
            };
        }
    }

}
