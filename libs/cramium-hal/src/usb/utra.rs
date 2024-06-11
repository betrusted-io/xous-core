use utralib::{Field, Register};

pub const DEVCAP: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0, 0xffffffff);
pub const DEVCAP_VESION: Field = Field::new(8, 0, DEVCAP);
pub const DEVCAP_EP_IN: Field = Field::new(4, 8, DEVCAP);
pub const DEVCAP_EP_OUT: Field = Field::new(4, 12, DEVCAP);
pub const DEVCAP_MAX_INTS: Field = Field::new(10, 16, DEVCAP);
pub const DEVCAP_GEN1: Field = Field::new(1, 27, DEVCAP);
pub const DEVCAP_GEN2: Field = Field::new(1, 28, DEVCAP);
pub const DEVCAP_ISOCH: Field = Field::new(1, 29, DEVCAP);

pub const DEVCONFIG: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x10 / 4, 0xFF);
pub const DEVCONFIG_MAX_SPEED: Field = Field::new(4, 0, DEVCONFIG);
pub const DEVCONFIG_USB3_DISABLE_COUNT: Field = Field::new(4, 4, DEVCONFIG);

pub const EVENTCONFIG: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x14 / 4, 0xFFFF_FFFF);
pub const EVENTCONFIG_CSC_ENABLE: Field = Field::new(1, 0, EVENTCONFIG);
pub const EVENTCONFIG_PEC_ENABLE: Field = Field::new(1, 1, EVENTCONFIG);
pub const EVENTCONFIG_PPC_ENABLE: Field = Field::new(1, 3, EVENTCONFIG);
pub const EVENTCONFIG_PRC_ENABLE: Field = Field::new(1, 4, EVENTCONFIG);
pub const EVENTCONFIG_PLC_ENABLE: Field = Field::new(1, 5, EVENTCONFIG);
pub const EVENTCONFIG_CEC_ENABLE: Field = Field::new(1, 6, EVENTCONFIG);
pub const EVENTCONFIG_U3_PLC_ENABLE: Field = Field::new(1, 8, EVENTCONFIG);
pub const EVENTCONFIG_L1_PLC_ENABLE: Field = Field::new(1, 9, EVENTCONFIG);
pub const EVENTCONFIG_U3_RESUME_PLC_ENABLE: Field = Field::new(1, 10, EVENTCONFIG);
pub const EVENTCONFIG_L1_RESUME_PLC_ENABLE: Field = Field::new(1, 11, EVENTCONFIG);
pub const EVENTCONFIG_INACTIVE_PLC_ENABLE: Field = Field::new(1, 12, EVENTCONFIG);
pub const EVENTCONFIG_USB3_RESUME_NO_PLC_ENABLE: Field = Field::new(1, 13, EVENTCONFIG);
pub const EVENTCONFIG_USB2_RESUME_NO_PLC_ENABLE: Field = Field::new(1, 14, EVENTCONFIG);
pub const EVENTCONFIG_SETUP_ENABLE: Field = Field::new(1, 16, EVENTCONFIG);
pub const EVENTCONFIG_STOPPED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 17, EVENTCONFIG);
pub const EVENTCONFIG_HALTED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 18, EVENTCONFIG);
pub const EVENTCONFIG_DISABLED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 19, EVENTCONFIG);
pub const EVENTCONFIG_DISABLE_EVENT_ENABLE: Field = Field::new(1, 20, EVENTCONFIG);

pub const USBCMD: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x20 / 4, 0xFFFF_FFFF);
pub const USBCMD_RUN_STOP: Field = Field::new(1, 0, USBCMD);
pub const USBCMD_SOFT_RESET: Field = Field::new(1, 1, USBCMD);
pub const USBCMD_INT_ENABLE: Field = Field::new(1, 2, USBCMD);
pub const USBCMD_SYS_ERR_ENABLE: Field = Field::new(1, 3, USBCMD);
pub const USBCMD_EWE: Field = Field::new(1, 10, USBCMD);
pub const USBCMD_FORCE_TERMINATION: Field = Field::new(1, 11, USBCMD);

pub const USBSTS: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x24 / 4, 0xFFFF_FFFF);
pub const USBSTS_CTL_HALTED: Field = Field::new(1, 0, USBSTS);
pub const USBSTS_SYSTEM_ERR: Field = Field::new(1, 2, USBSTS);
pub const USBSTS_EINT: Field = Field::new(1, 3, USBSTS);
pub const USBSTS_CTL_IDLE: Field = Field::new(1, 12, USBSTS);

pub const DCBAPLO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x28 / 4, 0xFFFF_FFFF);
pub const DBCAPLO_PTR_LO: Field = Field::new(26, 6, DCBAPLO);

pub const DCBAPHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x2C / 4, 0xFFFF_FFFF);
pub const DBCAPLO_PTR_HI: Field = Field::new(32, 0, DCBAPHI);

pub const PORTSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x30 / 4, 0xFFFF_FFFF);
pub const PORTSC_CCS: Field = Field::new(1, 0, PORTSC);
pub const PORTSC_PP: Field = Field::new(1, 3, PORTSC);
pub const PORTSC_PR: Field = Field::new(1, 4, PORTSC);
pub const PORTSC_PLS: Field = Field::new(4, 5, PORTSC);
pub const PORTSC_SPEED: Field = Field::new(4, 10, PORTSC);
pub const PORTSC_LWS: Field = Field::new(1, 16, PORTSC);
pub const PORTSC_CSC: Field = Field::new(1, 17, PORTSC);
pub const PORTSC_PPC: Field = Field::new(1, 20, PORTSC);
pub const PORTSC_PRC: Field = Field::new(1, 21, PORTSC);
pub const PORTSC_PLC: Field = Field::new(1, 22, PORTSC);
pub const PORTSC_CEC: Field = Field::new(1, 23, PORTSC);
pub const PORTSC_WCE: Field = Field::new(1, 25, PORTSC);
pub const PORTSC_WDE: Field = Field::new(1, 26, PORTSC);
pub const PORTSC_WPR: Field = Field::new(1, 31, PORTSC);

pub const U3PORTPMSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x34 / 4, 0xFFFF_FFFF);

pub const U2PORTPMSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x38 / 4, 0xFFFF_FFFF);

// pub const U3PORTLI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x3C / 4, 0xFFFF_FFFF);

pub const DOORBELL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x40 / 4, 0xFFFF_FFFF);
pub const DOORBELL_TARGET: Field = Field::new(5, 0, DOORBELL);

pub const MFINDEX: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x44 / 4, 0xFFFF_FFFF);
pub const MFINDEX_SYNC_EN: Field = Field::new(1, 0, MFINDEX);
pub const MFINDEX_OUT_OF_SYNC_EN: Field = Field::new(1, 1, MFINDEX);
pub const MFINDEX_IN_SYNC_EN: Field = Field::new(1, 2, MFINDEX);
pub const MFINDEX_INDEX_OUT_OF_SYNC_EN: Field = Field::new(1, 3, MFINDEX);
pub const MFINDEX_MFINDEX_EN: Field = Field::new(14, 4, MFINDEX);
pub const MFINDEX_MFOFFSET_EN: Field = Field::new(13, 18, MFINDEX);

pub const PTMCTRL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x48 / 4, 0xFFFF_FFFF);
pub const PTMCTRL_DELAY: Field = Field::new(14, 0, PTMCTRL);

pub const PTMSTS: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x4C / 4, 0xFFFF_FFFF);
pub const PTMSTS_MFINDEX_IN_SYNC: Field = Field::new(1, 2, PTMSTS);
pub const PTMSTS_MFINDEX: Field = Field::new(14, 4, PTMSTS);
pub const PTMSTS_MFOFFSET: Field = Field::new(13, 18, PTMSTS);

pub const EPENABLE: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x60 / 4, 0xFFFF_FFFF);
pub const EPENABLE_ENABLED: Field = Field::new(30, 2, EPENABLE);

pub const EPRUNNING: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x64 / 4, 0xFFFF_FFFF);
pub const EPRUNNING_RUNNING: Field = Field::new(30, 2, EPRUNNING);

pub const CMDPARA0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x70 / 4, 0xFFFF_FFFF);
pub const CMDPARA0_CMD0_INIT_EP0_DCS: Field = Field::new(1, 0, CMDPARA0);
pub const CMDPARA0_CMD1_UPDATE_EP0_MPS: Field = Field::new(16, 16, CMDPARA0);
pub const CMDPARA0_CMD2_SET_ADDR: Field = Field::new(8, 0, CMDPARA0);

pub const CMDPARA1: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x74 / 4, 0xFFFF_FFFF);

pub const CMDCTRL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x78 / 4, 0xFFFF_FFFF);
pub const CMDCTRL_ACTIVE: Field = Field::new(1, 0, CMDCTRL);
pub const CMDCTRL_IOC: Field = Field::new(1, 1, CMDCTRL);
pub const CMDCTRL_TYPE: Field = Field::new(4, 4, CMDCTRL);
pub const CMDCTRL_STATUS: Field = Field::new(4, 16, CMDCTRL);

pub const ODBCAP: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x80 / 4, 0xFFFF_FFFF);
pub const OBDCAP_RAM_SIZE: Field = Field::new(11, 0, ODBCAP);

pub const ODBCONFIG0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x90 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG0_EP0_OFFSET: Field = Field::new(10, 0, ODBCONFIG0);
pub const ODBCONFIG0_EP0_SIZE: Field = Field::new(3, 10, ODBCONFIG0);
pub const ODBCONFIG0_EP1_OFFSET: Field = Field::new(10, 16, ODBCONFIG0);
pub const ODBCONFIG0_EP1_SIZE: Field = Field::new(3, 26, ODBCONFIG0);

pub const ODBCONFIG1: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x94 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG1_EP2_OFFSET: Field = Field::new(10, 0, ODBCONFIG1);
pub const ODBCONFIG1_EP2_SIZE: Field = Field::new(3, 10, ODBCONFIG1);
pub const ODBCONFIG1_EP3_OFFSET: Field = Field::new(10, 16, ODBCONFIG1);
pub const ODBCONFIG1_EP3_SIZE: Field = Field::new(3, 26, ODBCONFIG1);

pub const ODBCONFIG2: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x98 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG2_EP4_OFFSET: Field = Field::new(10, 0, ODBCONFIG2);
pub const ODBCONFIG2_EP4_SIZE: Field = Field::new(3, 10, ODBCONFIG2);
pub const ODBCONFIG2_EP5_OFFSET: Field = Field::new(10, 16, ODBCONFIG2);
pub const ODBCONFIG2_EP5_SIZE: Field = Field::new(3, 26, ODBCONFIG2);

pub const ODBCONFIG3: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x9C / 4, 0xFFFF_FFFF);
pub const ODBCONFIG3_EP6_OFFSET: Field = Field::new(10, 0, ODBCONFIG3);
pub const ODBCONFIG3_EP6_SIZE: Field = Field::new(3, 10, ODBCONFIG3);
pub const ODBCONFIG3_EP7_OFFSET: Field = Field::new(10, 16, ODBCONFIG3);
pub const ODBCONFIG3_EP7_SIZE: Field = Field::new(3, 26, ODBCONFIG3);

pub const ODBCONFIG4: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA0 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG4_EP8_OFFSET: Field = Field::new(10, 0, ODBCONFIG4);
pub const ODBCONFIG4_EP8_SIZE: Field = Field::new(3, 10, ODBCONFIG4);
pub const ODBCONFIG4_EP9_OFFSET: Field = Field::new(10, 16, ODBCONFIG4);
pub const ODBCONFIG4_EP9_SIZE: Field = Field::new(3, 26, ODBCONFIG4);

pub const ODBCONFIG5: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA4 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG5_EP10_OFFSET: Field = Field::new(10, 0, ODBCONFIG5);
pub const ODBCONFIG5_EP10_SIZE: Field = Field::new(3, 10, ODBCONFIG5);
pub const ODBCONFIG5_EP11_OFFSET: Field = Field::new(10, 16, ODBCONFIG5);
pub const ODBCONFIG5_EP11_SIZE: Field = Field::new(3, 26, ODBCONFIG5);

pub const ODBCONFIG6: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA8 / 4, 0xFFFF_FFFF);
pub const ODBCONFIG6_EP12_OFFSET: Field = Field::new(10, 0, ODBCONFIG6);
pub const ODBCONFIG6_EP12_SIZE: Field = Field::new(3, 10, ODBCONFIG6);
pub const ODBCONFIG6_EP13_OFFSET: Field = Field::new(10, 16, ODBCONFIG6);
pub const ODBCONFIG6_EP13_SIZE: Field = Field::new(3, 26, ODBCONFIG6);

pub const ODBCONFIG7: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xAC / 4, 0xFFFF_FFFF);
pub const ODBCONFIG7_EP14_OFFSET: Field = Field::new(10, 0, ODBCONFIG7);
pub const ODBCONFIG7_EP14_SIZE: Field = Field::new(3, 10, ODBCONFIG7);
pub const ODBCONFIG7_EP15_OFFSET: Field = Field::new(10, 16, ODBCONFIG7);
pub const ODBCONFIG7_EP15_SIZE: Field = Field::new(3, 26, ODBCONFIG7);

pub const DEBUG0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xB0 / 4, 0xFFFF_FFFF);
pub const DEBUG0_DEV_ADDR: Field = Field::new(7, 0, DEBUG0);
pub const DEBUG0_NUMP_LIMIT: Field = Field::new(4, 8, DEBUG0);

pub const IMAN: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x100 / 4, 0xFFFF_FFFF);
pub const IMAN_IP: Field = Field::new(1, 0, IMAN);
pub const IMAN_IE: Field = Field::new(1, 1, IMAN);

pub const IMOD: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x104 / 4, 0xFFFF_FFFF);
pub const IMOD_MOD_INTERVAL: Field = Field::new(16, 0, IMOD);
pub const IMOD_MOD_COUNTER: Field = Field::new(16, 32, IMOD);

pub const ERSTSZ: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x108 / 4, 0xFFFF_FFFF);
pub const ERSTSZ_RING_SEG_TABLE: Field = Field::new(16, 0, ERSTSZ);

pub const ERSTBALO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x110 / 4, 0xFFFF_FFFF);
pub const ERSTBAL0_BASE_ADDR_LO: Field = Field::new(26, 6, ERSTBALO);

pub const ERSTBAHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x114 / 4, 0xFFFF_FFFF);
pub const ERSTBAHI_BASE_ADDR_HI: Field = Field::new(32, 0, ERSTBAHI);

pub const ERDPLO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x118 / 4, 0xFFFF_FFFF);
pub const ERDPLO_DESI: Field = Field::new(3, 0, ERDPLO);
pub const ERDPLO_EHB: Field = Field::new(1, 3, ERDPLO);
pub const ERDPLO_DQ_PTR: Field = Field::new(28, 4, ERDPLO);

pub const ERDPHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x11C / 4, 0xFFFF_FFFF);
pub const ERDPHI_DQ_PTR: Field = Field::new(32, 0, ERDPHI);

pub const CORIGINE_EVENT_RING_NUM: usize = 1;
pub const CORIGINE_USB_BASE: usize = 0x5020_2000;
pub const CORIGINE_DEV_OFFSET: usize = 0x400;
pub const CORIGINE_UICR_OFFSET: usize = CORIGINE_DEV_OFFSET + 0x100;
pub const CORIGINE_USB_LEN: usize = 0x3000;
