// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[allow(dead_code)]
pub enum VersionDescriptor {
    None = 0x0000,
    /// SAM (no version claimed)
    SAMNoVersionClaimed = 0x0020,
    /// SAM T10/0994-D revision 18
    SAMT100994DRevision18 = 0x003B,
    /// SAM ANSI INCITS 270-1996
    SamAnsiIncits2701996 = 0x003C,
    /// SAM-2 (no version claimed)
    SAM2NoVersionClaimed = 0x0040,
    /// SAM-2 T10/1157-D revision 23
    SAM2T101157DRevision23 = 0x0054,
    /// SAM-2 T10/1157-D revision 24
    SAM2T101157DRevision24 = 0x0055,
    /// SAM-2 ANSI INCITS 366-2003
    Sam2AnsiIncits3662003 = 0x005C,
    /// SAM-2 ISO/IEC 14776-412
    Sam2IsoIec14776412 = 0x005E,
    /// SAM-3 (no version claimed)
    SAM3NoVersionClaimed = 0x0060,
    /// SAM-3 T10/1561-D revision 7
    SAM3T101561DRevision7 = 0x0062,
    /// SAM-3 T10/1561-D revision 13
    SAM3T101561DRevision13 = 0x0075,
    /// SAM-3 T10/1561-D revision 14
    SAM3T101561DRevision14 = 0x0076,
    /// SAM-3 ANSI INCITS 402-2005
    Sam3AnsiIncits4022005 = 0x0077,
    /// SAM-4 (no version claimed)
    SAM4NoVersionClaimed = 0x0080,
    /// SAM-4 T10/1683-D revision 13
    SAM4T101683DRevision13 = 0x0087,
    /// SAM-4 T10/1683-D revision 14
    SAM4T101683DRevision14 = 0x008B,
    /// SAM-4 ANSI INCITS 447-2008
    Sam4AnsiIncits4472008 = 0x0090,
    /// SAM-4 ISO/IEC 14776-414
    Sam4IsoIec14776414 = 0x0092,
    /// SAM-5 (no version claimed)
    SAM5NoVersionClaimed = 0x00A0,
    /// SAM-5 T10/2104-D revision 4
    SAM5T102104DRevision4 = 0x00A2,
    /// SAM-5 T10/2104-D revision 20
    SAM5T102104DRevision20 = 0x00A4,
    /// SAM-5 T10/2104-D revision 21
    SAM5T102104DRevision21 = 0x00A6,
    /// SAM-5 ANSI INCITS 515-2016
    Sam5AnsiIncits5152016 = 0x00A8,
    /// SAM-6 (no version claimed)
    SAM6NoVersionClaimed = 0x00C0,
    /// SPC (no version claimed)
    SPCNoVersionClaimed = 0x0120,
    /// SPC T10/0995-D revision 11a
    SPCT100995DRevision11a = 0x013B,
    /// SPC ANSI INCITS 301-1997
    SpcAnsiIncits3011997 = 0x013C,
    /// MMC (no version claimed)
    MMCNoVersionClaimed = 0x0140,
    /// MMC T10/1048-D revision 10a
    MMCT101048DRevision10a = 0x015B,
    /// MMC ANSI INCITS 304-1997
    MmcAnsiIncits3041997 = 0x015C,
    /// SCC (no version claimed)
    SCCNoVersionClaimed = 0x0160,
    /// SCC T10/1047-D revision 06c
    SCCT101047DRevision06c = 0x017B,
    /// SCC ANSI INCITS 276-1997
    SccAnsiIncits2761997 = 0x017C,
    /// SBC (no version claimed)
    SBCNoVersionClaimed = 0x0180,
    /// SBC T10/0996-D revision 08c
    SBCT100996DRevision08c = 0x019B,
    /// SBC ANSI INCITS 306-1998
    SbcAnsiIncits3061998 = 0x019C,
    /// SMC (no version claimed)
    SMCNoVersionClaimed = 0x01A0,
    /// SMC T10/0999-D revision 10a
    SMCT100999DRevision10a = 0x01BB,
    /// SMC ANSI INCITS 314-1998
    SmcAnsiIncits3141998 = 0x01BC,
    /// SMC ISO/IEC 14776-351
    SmcIsoIec14776351 = 0x01BE,
    /// SES (no version claimed)
    SESNoVersionClaimed = 0x01C0,
    /// SES T10/1212-D revision 08b
    SEST101212DRevision08b = 0x01DB,
    /// SES ANSI INCITS 305-1998
    SesAnsiIncits3051998 = 0x01DC,
    /// SES T10/1212-D revision 08b w/ Amendment ANSI INCITS.305/AM1:2000
    SEST101212DRevision08bWAmendmentANSIINCITS305AM12000 = 0x01DD,
    /// SES ANSI INCITS 305-1998 w/ Amendment ANSI INCITS.305/AM1:2000
    SESANSIINCITS3051998WAmendmentANSIINCITS305AM12000 = 0x01DE,
    /// SCC-2 (no version claimed}
    SCC2NoVersionClaimed = 0x01E0,
    /// SCC-2 T10/1125-D revision 04
    SCC2T101125DRevision04 = 0x01FB,
    /// SCC-2 ANSI INCITS 318-1998
    Scc2AnsiIncits3181998 = 0x01FC,
    /// SSC (no version claimed)
    SSCNoVersionClaimed = 0x0200,
    /// SSC T10/0997-D revision 17
    SSCT100997DRevision17 = 0x0201,
    /// SSC T10/0997-D revision 22
    SSCT100997DRevision22 = 0x0207,
    /// SSC ANSI INCITS 335-2000
    SscAnsiIncits3352000 = 0x021C,
    /// RBC (no version claimed)
    RBCNoVersionClaimed = 0x0220,
    /// RBC T10/1240-D revision 10a
    RBCT101240DRevision10a = 0x0238,
    /// RBC ANSI INCITS 330-2000
    RbcAnsiIncits3302000 = 0x023C,
    /// MMC-2 (no version claimed)
    MMC2NoVersionClaimed = 0x0240,
    /// MMC-2 T10/1228-D revision 11
    MMC2T101228DRevision11 = 0x0255,
    /// MMC-2 T10/1228-D revision 11a
    MMC2T101228DRevision11a = 0x025B,
    /// MMC-2 ANSI INCITS 333-2000
    Mmc2AnsiIncits3332000 = 0x025C,
    /// SPC-2 (no version claimed)
    SPC2NoVersionClaimed = 0x0260,
    /// SPC-2 T10/1236-D revision 12
    SPC2T101236DRevision12 = 0x0267,
    /// SPC-2 T10/1236-D revision 18
    SPC2T101236DRevision18 = 0x0269,
    /// SPC-2 T10/1236-D revision 19
    SPC2T101236DRevision19 = 0x0275,
    /// SPC-2 T10/1236-D revision 20
    SPC2T101236DRevision20 = 0x0276,
    /// SPC-2 ANSI INCITS 351-2001
    Spc2AnsiIncits3512001 = 0x0277,
    /// SPC-2 ISO/IEC 14776-452
    Spc2IsoIec14776452 = 0x0278,
    /// OCRW (no version claimed)
    OCRWNoVersionClaimed = 0x0280,
    /// OCRW ISO/IEC 14776-381
    OcrwIsoIec14776381 = 0x029E,
    /// MMC-3 (no version claimed)
    MMC3NoVersionClaimed = 0x02A0,
    /// MMC-3 T10/1363-D revision 9
    MMC3T101363DRevision9 = 0x02B5,
    /// MMC-3 T10/1363-D revision 10g
    MMC3T101363DRevision10g = 0x02B6,
    /// MMC-3 ANSI INCITS 360-2002
    Mmc3AnsiIncits3602002 = 0x02B8,
    /// SMC-2 (no version claimed)
    SMC2NoVersionClaimed = 0x02E0,
    /// SMC-2 T10/1383-D revision 5
    SMC2T101383DRevision5 = 0x02F5,
    /// SMC-2 T10/1383-D revision 6
    SMC2T101383DRevision6 = 0x02FC,
    /// SMC-2 T10/1383-D revision 7
    SMC2T101383DRevision7 = 0x02FD,
    /// SMC-2 ANSI INCITS 382-2004
    Smc2AnsiIncits3822004 = 0x02FE,
    /// SPC-3 (no version claimed)
    SPC3NoVersionClaimed = 0x0300,
    /// SPC-3 T10/1416-D revision 7
    SPC3T101416DRevision7 = 0x0301,
    /// SPC-3 T10/1416-D revision 21
    SPC3T101416DRevision21 = 0x0307,
    /// SPC-3 T10/1416-D revision 22
    SPC3T101416DRevision22 = 0x030F,
    /// SPC-3 T10/1416-D revision 23
    SPC3T101416DRevision23 = 0x0312,
    /// SPC-3 ANSI INCITS 408-2005
    Spc3AnsiIncits4082005 = 0x0314,
    /// SPC-3 ISO/IEC 14776-453
    Spc3IsoIec14776453 = 0x0316,
    /// SBC-2 (no version claimed)
    SBC2NoVersionClaimed = 0x0320,
    /// SBC-2 T10/1417-D revision 5a
    SBC2T101417DRevision5a = 0x0322,
    /// SBC-2 T10/1417-D revision 15
    SBC2T101417DRevision15 = 0x0324,
    /// SBC-2 T10/1417-D revision 16
    SBC2T101417DRevision16 = 0x033B,
    /// SBC-2 ANSI INCITS 405-2005
    Sbc2AnsiIncits4052005 = 0x033D,
    /// SBC-2 ISO/IEC 14776-322
    Sbc2IsoIec14776322 = 0x033E,
    /// OSD (no version claimed)
    OSDNoVersionClaimed = 0x0340,
    /// OSD T10/1355-D revision 0
    OSDT101355DRevision0 = 0x0341,
    /// OSD T10/1355-D revision 7a
    OSDT101355DRevision7a = 0x0342,
    /// OSD T10/1355-D revision 8
    OSDT101355DRevision8 = 0x0343,
    /// OSD T10/1355-D revision 9
    OSDT101355DRevision9 = 0x0344,
    /// OSD T10/1355-D revision 10
    OSDT101355DRevision10 = 0x0355,
    /// OSD ANSI INCITS 400-2004
    OsdAnsiIncits4002004 = 0x0356,
    /// SSC-2 (no version claimed)
    SSC2NoVersionClaimed = 0x0360,
    /// SSC-2 T10/1434-D revision 7
    SSC2T101434DRevision7 = 0x0374,
    /// SSC-2 T10/1434-D revision 9
    SSC2T101434DRevision9 = 0x0375,
    /// SSC-2 ANSI INCITS 380-2003
    Ssc2AnsiIncits3802003 = 0x037D,
    /// BCC (no version claimed)
    BCCNoVersionClaimed = 0x0380,
    /// MMC-4 (no version claimed)
    MMC4NoVersionClaimed = 0x03A0,
    /// MMC-4 T10/1545-D revision 5"},
    MMC4T101545DRevision5 = 0x03B0,
    /// MMC-4 T10/1545-D revision 5a
    MMC4T101545DRevision5a = 0x03B1,
    /// MMC-4 T10/1545-D revision 3
    MMC4T101545DRevision3 = 0x03BD,
    /// MMC-4 T10/1545-D revision 3d
    MMC4T101545DRevision3d = 0x03BE,
    /// MMC-4 ANSI INCITS 401-2005
    Mmc4AnsiIncits4012005 = 0x03BF,
    /// ADC (no version claimed)
    ADCNoVersionClaimed = 0x03C0,
    /// ADC T10/1558-D revision 6
    ADCT101558DRevision6 = 0x03D5,
    /// ADC T10/1558-D revision 7
    ADCT101558DRevision7 = 0x03D6,
    /// ADC ANSI INCITS 403-2005
    AdcAnsiIncits4032005 = 0x03D7,
    /// SES-2 (no version claimed)
    SES2NoVersionClaimed = 0x03E0,
    /// SES-2 T10/1559-D revision 16
    SES2T101559DRevision16 = 0x03E1,
    /// SES-2 T10/1559-D revision 19
    SES2T101559DRevision19 = 0x03E7,
    /// SES-2 T10/1559-D revision 20
    SES2T101559DRevision20 = 0x03EB,
    /// SES-2 ANSI INCITS 448-2008
    Ses2AnsiIncits4482008 = 0x03F0,
    /// SES-2 ISO/IEC 14776-372
    Ses2IsoIec14776372 = 0x03F2,
    /// SSC-3 (no version claimed)
    SSC3NoVersionClaimed = 0x0400,
    /// SSC-3 T10/1611-D revision 04a
    SSC3T101611DRevision04a = 0x0403,
    /// SSC-3 T10/1611-D revision 05
    SSC3T101611DRevision05 = 0x0407,
    /// SSC-3 ANSI INCITS 467-2011
    Ssc3AnsiIncits4672011 = 0x0409,
    /// SSC-3 ISO/IEC 14776-333:2013
    Ssc3IsoIec147763332013 = 0x040B,
    /// MMC-5 (no version claimed)
    MMC5NoVersionClaimed = 0x0420,
    /// MMC-5 T10/1675-D revision 03
    MMC5T101675DRevision03 = 0x042F,
    /// MMC-5 T10/1675-D revision 03b
    MMC5T101675DRevision03b = 0x0431,
    /// MMC-5 T10/1675-D revision 04
    MMC5T101675DRevision04 = 0x0432,
    /// MMC-5 ANSI INCITS 430-2007
    Mmc5AnsiIncits4302007 = 0x0434,
    /// OSD-2 (no version claimed)
    OSD2NoVersionClaimed = 0x0440,
    /// OSD-2 T10/1729-D revision 4
    OSD2T101729DRevision4 = 0x0444,
    /// OSD-2 T10/1729-D revision 5
    OSD2T101729DRevision5 = 0x0446,
    /// OSD-2 ANSI INCITS 458-2011
    Osd2AnsiIncits4582011 = 0x0448,
    /// SPC-4 (no version claimed)
    SPC4NoVersionClaimed = 0x0460,
    /// SPC-4 T10/BSR INCITS 513 revision 16
    SPC4T10BSRINCITS513Revision16 = 0x0461,
    /// SPC-4 T10/BSR INCITS 513 revision 18
    SPC4T10BSRINCITS513Revision18 = 0x0462,
    /// SPC-4 T10/BSR INCITS 513 revision 23
    SPC4T10BSRINCITS513Revision23 = 0x0463,
    /// SPC-4 T10/BSR INCITS 513 revision 36
    SPC4T10BSRINCITS513Revision36 = 0x0466,
    /// SPC-4 T10/BSR INCITS 513 revision 37
    SPC4T10BSRINCITS513Revision37 = 0x0468,
    /// SPC-4 T10/BSR INCITS 513 revision 37a
    SPC4T10BSRINCITS513Revision37a = 0x0469,
    /// SPC-4 ANSI INCITS 513-2015
    Spc4AnsiIncits5132015 = 0x046C,
    /// SMC-3 (no version claimed)
    SMC3NoVersionClaimed = 0x0480,
    /// SMC-3 T10/1730-D revision 15
    SMC3T101730DRevision15 = 0x0482,
    /// SMC-3 T10/1730-D revision 16
    SMC3T101730DRevision16 = 0x0484,
    /// SMC-3 ANSI INCITS 484-2012
    Smc3AnsiIncits4842012 = 0x0486,
    /// ADC-2 (no version claimed)
    ADC2NoVersionClaimed = 0x04A0,
    /// ADC-2 T10/1741-D revision 7
    ADC2T101741DRevision7 = 0x04A7,
    /// ADC-2 T10/1741-D revision 8
    ADC2T101741DRevision8 = 0x04AA,
    /// ADC-2 ANSI INCITS 441-2008
    Adc2AnsiIncits4412008 = 0x04AC,
    /// SBC-3 (no version claimed)
    SBC3NoVersionClaimed = 0x04C0,
    /// SBC-3 T10/BSR INCITS 514 revision 35
    SBC3T10BSRINCITS514Revision35 = 0x04C3,
    /// SBC-3 T10/BSR INCITS 514 revision 36
    SBC3T10BSRINCITS514Revision36 = 0x04C5,
    /// SBC-3 ANSI INCITS 514-2014
    Sbc3AnsiIncits5142014 = 0x04C8,
    /// MMC-6 (no version claimed)
    MMC6NoVersionClaimed = 0x04E0,
    /// MMC-6 T10/1836-D revision 2b
    MMC6T101836DRevision2b = 0x04E3,
    /// MMC-6 T10/1836-D revision 02g
    MMC6T101836DRevision02g = 0x04E5,
    /// MMC-6 ANSI INCITS 468-2010
    Mmc6AnsiIncits4682010 = 0x04E6,
    /// MMC-6 ANSI INCITS 468-2010 + MMC-6/AM1 ANSI INCITS 468-2010/AM 1
    Mmc6AnsiIncits4682010Mmc6Am1AnsiIncits4682010Am1 = 0x04E7,
    /// ADC-3 (no version claimed)
    ADC3NoVersionClaimed = 0x0500,
    /// ADC-3 T10/1895-D revision 04
    ADC3T101895DRevision04 = 0x0502,
    /// ADC-3 T10/1895-D revision 05
    ADC3T101895DRevision05 = 0x0504,
    /// ADC-3 T10/1895-D revision 05a
    ADC3T101895DRevision05a = 0x0506,
    /// ADC-3 ANSI INCITS 497-2012
    Adc3AnsiIncits4972012 = 0x050A,
    /// SSC-4 (no version claimed)
    SSC4NoVersionClaimed = 0x0520,
    /// SSC-4 T10/BSR INCITS 516 revision 2
    SSC4T10BSRINCITS516Revision2 = 0x0523,
    /// SSC-4 T10/BSR INCITS 516 revision 3
    SSC4T10BSRINCITS516Revision3 = 0x0525,
    /// SSC-4 SSC-4 ANSI INCITS 516-2013
    Ssc4Ssc4AnsiIncits5162013 = 0x0527,
    /// OSD-3 (no version claimed)
    OSD3NoVersionClaimed = 0x0560,
    /// SES-3 (no version claimed)
    SES3NoVersionClaimed = 0x0580,
    /// SES-3 T10/BSR INCITS 518 revision 13
    SES3T10BSRINCITS518Revision13 = 0x0582,
    /// SES-3 T10/BSR INCITS 518 revision 14
    SES3T10BSRINCITS518Revision14 = 0x0584,
    /// SSC-5 (no version claimed)
    SSC5NoVersionClaimed = 0x05A0,
    /// SPC-5 (no version claimed)
    SPC5NoVersionClaimed = 0x05C0,
    /// SFSC (no version claimed)
    SFSCNoVersionClaimed = 0x05E0,
    /// SFSC BSR INCITS 501 revision 01
    SFSCBSRINCITS501Revision01 = 0x05E3,
    /// SFSC BSR INCITS 501 revision 02
    SFSCBSRINCITS501Revision02 = 0x05E5,
    /// SFSC ANSI INCITS 501-2016
    SfscAnsiIncits5012016 = 0x05E8,
    /// SBC-4 (no version claimed)
    SBC4NoVersionClaimed = 0x0600,
    /// ZBC (no version claimed)
    ZBCNoVersionClaimed = 0x0620,
    /// ZBC BSR INCITS 536 revision 02
    ZBCBSRINCITS536Revision02 = 0x0622,
    /// ZBC BSR INCITS 536 revision 05
    ZBCBSRINCITS536Revision05 = 0x0624,
    /// ADC-4 (no version claimed)
    ADC4NoVersionClaimed = 0x0640,
    /// ZBC-2 (no version claimed)
    ZBC2NoVersionClaimed = 0x0660,
    /// SES-4 (no version claimed)
    SES4NoVersionClaimed = 0x0680,
    /// SSA-TL2 (no version claimed)
    SSATL2NoVersionClaimed = 0x0820,
    /// SSA-TL2 T10/1147-D revision 05b
    SSATL2T101147DRevision05b = 0x083B,
    /// SSA-TL2 ANSI INCITS 308-1998
    SsaTl2AnsiIncits3081998 = 0x083C,
    /// SSA-TL1 (no version claimed)
    SSATL1NoVersionClaimed = 0x0840,
    /// SSA-TL1 T10/0989-D revision 10b
    SSATL1T100989DRevision10b = 0x085B,
    /// SSA-TL1 ANSI INCITS 295-1996
    SsaTl1AnsiIncits2951996 = 0x085C,
    /// SSA-S3P (no version claimed)
    SSAS3PNoVersionClaimed = 0x0860,
    /// SSA-S3P T10/1051-D revision 05b
    SSAS3PT101051DRevision05b = 0x087B,
    /// SSA-S3P ANSI INCITS 309-1998
    SsaS3pAnsiIncits3091998 = 0x087C,
    /// SSA-S2P (no version claimed)
    SSAS2PNoVersionClaimed = 0x0880,
    /// SSA-S2P T10/1121-D revision 07b
    SSAS2PT101121DRevision07b = 0x089B,
    /// SSA-S2P ANSI INCITS 294-1996
    SsaS2pAnsiIncits2941996 = 0x089C,
    /// SIP (no version claimed)
    SIPNoVersionClaimed = 0x08A0,
    /// SIP T10/0856-D revision 10
    SIPT100856DRevision10 = 0x08BB,
    /// SIP ANSI INCITS 292-1997
    SipAnsiIncits2921997 = 0x08BC,
    /// FCP (no version claimed)
    FCPNoVersionClaimed = 0x08C0,
    /// FCP T10/0856-D revision 12
    FCPT100856DRevision12 = 0x08DB,
    /// FCP ANSI INCITS 269-1996
    FcpAnsiIncits2691996 = 0x08DC,
    /// SBP-2 (no version claimed)
    SBP2NoVersionClaimed = 0x08E0,
    /// SBP-2 T10/1155-D revision 04
    SBP2T101155DRevision04 = 0x08FB,
    /// SBP-2 ANSI INCITS 325-1999
    Sbp2AnsiIncits3251999 = 0x08FC,
    /// FCP-2 (no version claimed)
    FCP2NoVersionClaimed = 0x0900,
    /// FCP-2 T10/1144-D revision 4
    FCP2T101144DRevision4 = 0x0901,
    /// FCP-2 T10/1144-D revision 7
    FCP2T101144DRevision7 = 0x0915,
    /// FCP-2 T10/1144-D revision 7a
    FCP2T101144DRevision7a = 0x0916,
    /// FCP-2 ANSI INCITS 350-2003
    Fcp2AnsiIncits3502003 = 0x0917,
    /// FCP-2 T10/1144-D revision 8
    FCP2T101144DRevision8 = 0x0918,
    /// SST (no version claimed)
    SSTNoVersionClaimed = 0x0920,
    /// SST T10/1380-D revision 8b
    SSTT101380DRevision8b = 0x0935,
    /// SRP (no version claimed)
    SRPNoVersionClaimed = 0x0940,
    /// SRP T10/1415-D revision 10
    SRPT101415DRevision10 = 0x0954,
    /// SRP T10/1415-D revision 16a
    SRPT101415DRevision16a = 0x0955,
    /// SRP ANSI INCITS 365-2002
    SrpAnsiIncits3652002 = 0x095C,
    /// iSCSI (no version claimed)
    ISCSINoVersionClaimed = 0x0960,
    /// iSCSI RFC 7143
    ISCSIRFC7143 = 0x0961,
    /// iSCSI RFC 7144
    ISCSIRFC7144 = 0x0962,
    /// SBP-3 (no version claimed)
    SBP3NoVersionClaimed = 0x0980,
    /// SBP-3 T10/1467-D revision 1f
    SBP3T101467DRevision1f = 0x0982,
    /// SBP-3 T10/1467-D revision 3
    SBP3T101467DRevision3 = 0x0994,
    /// SBP-3 T10/1467-D revision 4
    SBP3T101467DRevision4 = 0x099A,
    /// SBP-3 T10/1467-D revision 5
    SBP3T101467DRevision5 = 0x099B,
    /// SBP-3 ANSI INCITS 375-2004
    Sbp3AnsiIncits3752004 = 0x099C,
    /// SRP-2 (no version claimed)
    SRP2NoVersionClaimed = 0x09A0,
    /// ADP (no version claimed)
    ADPNoVersionClaimed = 0x09C0,
    /// ADT (no version claimed)
    ADTNoVersionClaimed = 0x09E0,
    /// ADT T10/1557-D revision 11
    ADTT101557DRevision11 = 0x09F9,
    /// ADT T10/1557-D revision 14
    ADTT101557DRevision14 = 0x09FA,
    /// ADT ANSI INCITS 406-2005
    AdtAnsiIncits4062005 = 0x09FD,
    /// FCP-3 (no version claimed)
    FCP3NoVersionClaimed = 0x0A00,
    /// FCP-3 T10/1560-D revision 3f
    FCP3T101560DRevision3f = 0x0A07,
    /// FCP-3 T10/1560-D revision 4
    FCP3T101560DRevision4 = 0x0A0F,
    /// FCP-3 ANSI INCITS 416-2006
    Fcp3AnsiIncits4162006 = 0x0A11,
    /// FCP-3 ISO/IEC 14776-223
    Fcp3IsoIec14776223 = 0x0A1C,
    /// ADT-2 (no version claimed)
    ADT2NoVersionClaimed = 0x0A20,
    /// ADT-2 T10/1742-D revision 06
    ADT2T101742DRevision06 = 0x0A22,
    /// ADT-2 T10/1742-D revision 08
    ADT2T101742DRevision08 = 0x0A27,
    /// ADT-2 T10/1742-D revision 09
    ADT2T101742DRevision09 = 0x0A28,
    /// ADT-2 ANSI INCITS 472-2011
    Adt2AnsiIncits4722011 = 0x0A2B,
    /// FCP-4 (no version claimed)
    FCP4NoVersionClaimed = 0x0A40,
    /// FCP-4 T10/1828-D revision 01
    FCP4T101828DRevision01 = 0x0A42,
    /// FCP-4 T10/1828-D revision 02
    FCP4T101828DRevision02 = 0x0A44,
    /// FCP-4 T10/1828-D revision 02b
    FCP4T101828DRevision02b = 0x0A45,
    /// FCP-4 ANSI INCITS 481-2012
    Fcp4AnsiIncits4812012 = 0x0A46,
    /// ADT-3 (no version claimed)
    ADT3NoVersionClaimed = 0x0A60,
    /// SPI (no version claimed)
    SPINoVersionClaimed = 0x0AA0,
    /// SPI T10/0855-D revision 15a
    SPIT100855DRevision15a = 0x0AB9,
    /// SPI ANSI INCITS 253-1995
    SpiAnsiIncits2531995 = 0x0ABA,
    /// SPI T10/0855-D revision 15a with SPI Amnd revision 3a
    SPIT100855DRevision15aWithSPIAmndRevision3a = 0x0ABB,
    /// SPI ANSI INCITS 253-1995 with SPI Amnd ANSI INCITS 253/AM1:1998
    SPIANSIINCITS2531995WithSPIAmndANSIINCITS253AM11998 = 0x0ABC,
    /// Fast-20 (no version claimed)
    Fast20NoVersionClaimed = 0x0AC0,
    /// Fast-20 T10/1071-D revision 06
    Fast20T101071DRevision06 = 0x0ADB,
    /// Fast-20 ANSI INCITS 277-1996
    Fast20ANSIINCITS2771996 = 0x0ADC,
    /// SPI-2 (no version claimed)
    SPI2NoVersionClaimed = 0x0AE0,
    /// SPI-2 T10/1142-D revision 20b
    SPI2T101142DRevision20b = 0x0AFB,
    /// SPI-2 ANSI INCITS 302-1999
    Spi2AnsiIncits3021999 = 0x0AFC,
    /// SPI-3 (no version claimed)
    SPI3NoVersionClaimed = 0x0B00,
    /// SPI-3 T10/1302-D revision 10
    SPI3T101302DRevision10 = 0x0B18,
    /// SPI-3 T10/1302-D revision 13a
    SPI3T101302DRevision13a = 0x0B19,
    /// SPI-3 T10/1302-D revision 14
    SPI3T101302DRevision14 = 0x0B1A,
    /// SPI-3 ANSI INCITS 336-2000
    Spi3AnsiIncits3362000 = 0x0B1C,
    /// EPI (no version claimed)
    EPINoVersionClaimed = 0x0B20,
    /// EPI T10/1134-D revision 16
    EPIT101134DRevision16 = 0x0B3B,
    /// EPI ANSI INCITS TR-23 1999
    EpiAnsiIncitsTr231999 = 0x0B3C,
    /// SPI-4 (no version claimed)
    SPI4NoVersionClaimed = 0x0B40,
    /// SPI-4 T10/1365-D revision 7
    SPI4T101365DRevision7 = 0x0B54,
    /// SPI-4 T10/1365-D revision 9
    SPI4T101365DRevision9 = 0x0B55,
    /// SPI-4 ANSI INCITS 362-2002
    Spi4AnsiIncits3622002 = 0x0B56,
    /// SPI-4 T10/1365-D revision 10
    SPI4T101365DRevision10 = 0x0B59,
    /// SPI-5 (no version claimed)
    SPI5NoVersionClaimed = 0x0B60,
    /// SPI-5 T10/1525-D revision 3
    SPI5T101525DRevision3 = 0x0B79,
    /// SPI-5 T10/1525-D revision 5
    SPI5T101525DRevision5 = 0x0B7A,
    /// SPI-5 T10/1525-D revision 6
    SPI5T101525DRevision6 = 0x0B7B,
    /// SPI-5 ANSI INCITS 367-2004
    Spi5AnsiIncits3672004 = 0x0B7C,
    /// SAS (no version claimed)
    SASNoVersionClaimed = 0x0BE0,
    /// SAS T10/1562-D revision 01
    SAST101562DRevision01 = 0x0BE1,
    /// SAS T10/1562-D revision 03
    SAST101562DRevision03 = 0x0BF5,
    /// SAS T10/1562-D revision 04
    SAST101562DRevision04A = 0x0BFA,
    /// SAS T10/1562-D revision 04
    SAST101562DRevision04B = 0x0BFB,
    /// SAS T10/1562-D revision 05
    SAST101562DRevision05 = 0x0BFC,
    /// SAS ANSI INCITS 376-2003
    SasAnsiIncits3762003 = 0x0BFD,
    /// SAS-1.1 (no version claimed)
    SAS11NoVersionClaimed = 0x0C00,
    /// SAS-1.1 T10/1602-D revision 9
    SAS11T101602DRevision9 = 0x0C07,
    /// SAS-1.1 T10/1602-D revision 10
    SAS11T101602DRevision10 = 0x0C0F,
    /// SAS-1.1 ANSI INCITS 417-2006
    Sas11AnsiIncits4172006 = 0x0C11,
    /// SAS-1.1 ISO/IEC 14776-151
    Sas11IsoIec14776151 = 0x0C12,
    /// SAS-2 (no version claimed)
    SAS2NoVersionClaimed = 0x0C20,
    /// SAS-2 T10/1760-D revision 14
    SAS2T101760DRevision14 = 0x0C23,
    /// SAS-2 T10/1760-D revision 15
    SAS2T101760DRevision15 = 0x0C27,
    /// SAS-2 T10/1760-D revision 16
    SAS2T101760DRevision16 = 0x0C28,
    /// SAS-2 ANSI INCITS 457-2010
    Sas2AnsiIncits4572010 = 0x0C2A,
    /// SAS-2.1 (no version claimed)
    SAS21NoVersionClaimed = 0x0C40,
    /// SAS-2.1 T10/2125-D revision 04
    SAS21T102125DRevision04 = 0x0C48,
    /// SAS-2.1 T10/2125-D revision 06
    SAS21T102125DRevision06 = 0x0C4A,
    /// SAS-2.1 T10/2125-D revision 07
    SAS21T102125DRevision07 = 0x0C4B,
    /// SAS-2.1 ANSI INCITS 478-2011
    Sas21AnsiIncits4782011 = 0x0C4E,
    /// SAS-2.1 ANSI INCITS 478-2011 w/ Amnd 1 ANSI INCITS 478/AM1-2014
    SAS21ANSIINCITS4782011WAmnd1ANSIINCITS478AM12014 = 0x0C4F,
    /// SAS-2.1 ISO/IEC 14776-153
    Sas21IsoIec14776153 = 0x0C52,
    /// SAS-3 (no version claimed)
    SAS3NoVersionClaimed = 0x0C60,
    /// SAS-3 T10/BSR INCITS 519 revision 05a
    SAS3T10BSRINCITS519Revision05a = 0x0C63,
    /// SAS-3 T10/BSR INCITS 519 revision 06
    SAS3T10BSRINCITS519Revision06 = 0x0C65,
    /// SAS-3 ANSI INCITS 519-2014
    Sas3AnsiIncits5192014 = 0x0C68,
    /// SAS-4 (no version claimed)
    SAS4NoVersionClaimed = 0x0C80,
    /// SAS-4 T10/BSR INCITS 534 revision 08a
    SAS4T10BSRINCITS534Revision08a = 0x0C82,
    /// FC-PH (no version claimed)
    FCPHNoVersionClaimed = 0x0D20,
    /// FC-PH ANSI INCITS 230-1994
    FcPhAnsiIncits2301994 = 0x0D3B,
    /// FC-PH ANSI INCITS 230-1994 with Amnd 1 ANSI INCITS 230/AM1:1996
    FCPHANSIINCITS2301994WithAmnd1ANSIINCITS230AM11996 = 0x0D3C,
    /// FC-AL (no version claimed)
    FCALNoVersionClaimed = 0x0D40,
    /// FC-AL ANSI INCITS 272-1996
    FcAlAnsiIncits2721996 = 0x0D5C,
    /// FC-AL-2 (no version claimed)
    FCAL2NoVersionClaimed = 0x0D60,
    /// FC-AL-2 T11/1133-D revision 7.0
    FCAL2T111133DRevision70 = 0x0D61,
    /// FC-AL-2 ANSI INCITS 332-1999 with AM1-2003 & AM2-2006
    FCAL2ANSIINCITS3321999WithAM12003AM22006 = 0x0D63,
    /// FC-AL-2 ANSI INCITS 332-1999 with Amnd 2 AM2-2006
    FCAL2ANSIINCITS3321999WithAmnd2AM22006 = 0x0D64,
    /// FC-AL-2 ISO/IEC 14165-122 with AM1 & AM2
    FCAL2ISOIEC14165122WithAM1AM2 = 0x0D65,
    /// FC-AL-2 ANSI INCITS 332-1999
    FcAl2AnsiIncits3321999 = 0x0D7C,
    /// FC-AL-2 ANSI INCITS 332-1999 with Amnd 1 AM1:2002
    FCAL2ANSIINCITS3321999WithAmnd1AM12002 = 0x0D7D,
    /// FC-PH-3 (no version claimed)
    FCPH3NoVersionClaimed = 0x0D80,
    /// FC-PH-3 ANSI INCITS 303-1998
    FcPh3AnsiIncits3031998 = 0x0D9C,
    /// FC-FS (no version claimed)
    FCFSNoVersionClaimed = 0x0DA0,
    /// FC-FS T11/1331-D revision 1.2
    FCFST111331DRevision12 = 0x0DB7,
    /// FC-FS T11/1331-D revision 1.7
    FCFST111331DRevision17 = 0x0DB8,
    /// FC-FS ANSI INCITS 373-2003
    FcFsAnsiIncits3732003 = 0x0DBC,
    /// FC-FS ISO/IEC 14165-251
    FcFsIsoIec14165251 = 0x0DBD,
    /// FC-PI (no version claimed)
    FCPINoVersionClaimed = 0x0DC0,
    /// FC-PI ANSI INCITS 352-2002
    FcPiAnsiIncits3522002 = 0x0DDC,
    /// FC-PI-2 (no version claimed)
    FCPI2NoVersionClaimed = 0x0DE0,
    /// FC-PI-2 T11/1506-D revision 5.0
    FCPI2T111506DRevision50 = 0x0DE2,
    /// FC-PI-2 ANSI INCITS 404-2006
    FcPi2AnsiIncits4042006 = 0x0DE4,
    /// FC-FS-2 (no version claimed)
    FCFS2NoVersionClaimed = 0x0E00,
    /// FC-FS-2 ANSI INCITS 242-2007
    FcFs2AnsiIncits2422007 = 0x0E02,
    /// FC-FS-2 ANSI INCITS 242-2007 with AM1 ANSI INCITS 242/AM1-2007
    FCFS2ANSIINCITS2422007WithAM1ANSIINCITS242AM12007 = 0x0E03,
    /// FC-LS (no version claimed)
    FCLSNoVersionClaimed = 0x0E20,
    /// FC-LS T11/1620-D revision 1.62
    FCLST111620DRevision162 = 0x0E21,
    /// FC-LS ANSI INCITS 433-2007
    FcLsAnsiIncits4332007 = 0x0E29,
    /// FC-SP (no version claimed)
    FCSPNoVersionClaimed = 0x0E40,
    /// FC-SP T11/1570-D revision 1.6
    FCSPT111570DRevision16 = 0x0E42,
    /// FC-SP ANSI INCITS 426-2007
    FcSpAnsiIncits4262007 = 0x0E45,
    /// FC-PI-3 (no version claimed)
    FCPI3NoVersionClaimed = 0x0E60,
    /// FC-PI-3 T11/1625-D revision 2.0
    FCPI3T111625DRevision20 = 0x0E62,
    /// FC-PI-3 T11/1625-D revision 2.1
    FCPI3T111625DRevision21 = 0x0E68,
    /// FC-PI-3 T11/1625-D revision 4.0
    FCPI3T111625DRevision40 = 0x0E6A,
    /// FC-PI-3 ANSI INCITS 460-2011
    FcPi3AnsiIncits4602011 = 0x0E6E,
    /// FC-PI-4 (no version claimed)
    FCPI4NoVersionClaimed = 0x0E80,
    /// FC-PI-4 T11/1647-D revision 8.0
    FCPI4T111647DRevision80 = 0x0E82,
    /// FC-PI-4 ANSI INCITS 450 -2009
    FcPi4AnsiIncits4502009 = 0x0E88,
    /// FC 10GFC (no version claimed)
    FC10GFCNoVersionClaimed = 0x0EA0,
    /// FC 10GFC ANSI INCITS 364-2003
    Fc10gfcAnsiIncits3642003 = 0x0EA2,
    /// FC 10GFC ISO/IEC 14165-116
    Fc10gfcIsoIec14165116 = 0x0EA3,
    /// FC 10GFC ISO/IEC 14165-116 with AM1
    FC10GFCISOIEC14165116WithAM1 = 0x0EA5,
    /// FC 10GFC ANSI INCITS 364-2003 with AM1 ANSI INCITS 364/AM1-2007
    FC10GFCANSIINCITS3642003WithAM1ANSIINCITS364AM12007 = 0x0EA6,
    /// FC-SP-2 (no version claimed)
    FCSP2NoVersionClaimed = 0x0EC0,
    /// FC-FS-3 (no version claimed)
    FCFS3NoVersionClaimed = 0x0EE0,
    /// FC-FS-3 T11/1861-D revision 0.9
    FCFS3T111861DRevision09 = 0x0EE2,
    /// FC-FS-3 T11/1861-D revision 1.0
    FCFS3T111861DRevision10 = 0x0EE7,
    /// FC-FS-3 T11/1861-D revision 1.10
    FCFS3T111861DRevision110 = 0x0EE9,
    /// FC-FS-3 ANSI INCITS 470-2011
    FcFs3AnsiIncits4702011 = 0x0EEB,
    /// FC-LS-2 (no version claimed)
    FCLS2NoVersionClaimed = 0x0F00,
    /// FC-LS-2 T11/2103-D revision 2.11
    FCLS2T112103DRevision211 = 0x0F03,
    /// FC-LS-2 T11/2103-D revision 2.21
    FCLS2T112103DRevision221 = 0x0F05,
    /// FC-LS-2 ANSI INCITS 477-2011
    FcLs2AnsiIncits4772011 = 0x0F07,
    /// FC-PI-5 (no version claimed)
    FCPI5NoVersionClaimed = 0x0F20,
    /// FC-PI-5 T11/2118-D revision 2.00
    FCPI5T112118DRevision200 = 0x0F27,
    /// FC-PI-5 T11/2118-D revision 3.00
    FCPI5T112118DRevision300 = 0x0F28,
    /// FC-PI-5 T11/2118-D revision 6.00
    FCPI5T112118DRevision600 = 0x0F2A,
    /// FC-PI-5 T11/2118-D revision 6.10
    FCPI5T112118DRevision610 = 0x0F2B,
    /// FC-PI-5 ANSI INCITS 479-2011
    FcPi5AnsiIncits4792011 = 0x0F2E,
    /// FC-PI-6 (no version claimed)
    FCPI6NoVersionClaimed = 0x0F40,
    /// FC-FS-4 (no version claimed)
    FCFS4NoVersionClaimed = 0x0F60,
    /// FC-LS-3 (no version claimed)
    FCLS3NoVersionClaimed = 0x0F80,
    /// FC-SCM (no version claimed)
    FCSCMNoVersionClaimed = 0x12A0,
    /// FC-SCM T11/1824DT revision 1.0
    FCSCMT111824DTRevision10 = 0x12A3,
    /// FC-SCM T11/1824DT revision 1.1
    FCSCMT111824DTRevision11 = 0x12A5,
    /// FC-SCM T11/1824DT revision 1.4
    FCSCMT111824DTRevision14 = 0x12A7,
    /// FC-SCM INCITS TR-47 2012
    FcScmIncitsTr472012 = 0x12AA,
    /// FC-DA-2 (no version claimed)
    FCDA2NoVersionClaimed = 0x12C0,
    /// FC-DA-2 T11/1870DT revision 1.04
    FCDA2T111870DTRevision104 = 0x12C3,
    /// FC-DA-2 T11/1870DT revision 1.06
    FCDA2T111870DTRevision106 = 0x12C5,
    /// FC-DA-2 INCITS TR-49 2012
    FcDa2IncitsTr492012 = 0x12C9,
    /// FC-DA (no version claimed)
    FCDANoVersionClaimed = 0x12E0,
    /// FC-DA T11/1513-DT revision 3.1
    FCDAT111513DTRevision31 = 0x12E2,
    /// FC-DA ANSI INCITS TR-36 2004
    FcDaAnsiIncitsTr362004 = 0x12E8,
    /// FC-DA ISO/IEC 14165-341
    FcDaIsoIec14165341 = 0x12E9,
    /// FC-Tape (no version claimed)
    FCTapeNoVersionClaimed = 0x1300,
    /// FC-Tape T11/1315-D revision 1.16
    FCTapeT111315DRevision116 = 0x1301,
    /// FC-Tape T11/1315-D revision 1.17
    FCTapeT111315DRevision117 = 0x131B,
    /// FC-Tape ANSI INCITS TR-24 1999
    FCTapeANSIINCITSTR241999 = 0x131C,
    /// FC-FLA (no version claimed)
    FCFLANoVersionClaimed = 0x1320,
    /// FC-FLA T11/1235-D revision 7
    FCFLAT111235DRevision7 = 0x133B,
    /// FC-FLA ANSI INCITS TR-20 1998
    FcFlaAnsiIncitsTr201998 = 0x133C,
    /// FC-PLDA (no version claimed)
    FCPLDANoVersionClaimed = 0x1340,
    /// FC-PLDA T11/1162-D revision 2.1
    FCPLDAT111162DRevision21 = 0x135B,
    /// FC-PLDA ANSI INCITS TR-19 1998
    FcPldaAnsiIncitsTr191998 = 0x135C,
    /// SSA-PH2 (no version claimed)
    SSAPH2NoVersionClaimed = 0x1360,
    /// SSA-PH2 T10/1145-D revision 09c
    SSAPH2T101145DRevision09c = 0x137B,
    /// SSA-PH2 ANSI INCITS 293-1996
    SsaPh2AnsiIncits2931996 = 0x137C,
    /// SSA-PH3 (no version claimed)
    SSAPH3NoVersionClaimed = 0x1380,
    /// SSA-PH3 T10/1146-D revision 05b
    SSAPH3T101146DRevision05b = 0x139B,
    /// SSA-PH3 ANSI INCITS 307-1998
    SsaPh3AnsiIncits3071998 = 0x139C,
    /// IEEE 1394 (no version claimed)
    IEEE1394NoVersionClaimed = 0x14A0,
    /// ANSI IEEE 1394:1995
    AnsiIeee13941995 = 0x14BD,
    /// IEEE 1394a (no version claimed)
    IEEE1394aNoVersionClaimed = 0x14C0,
    /// IEEE 1394b (no version claimed)
    IEEE1394bNoVersionClaimed = 0x14E0,
    /// ATA/ATAPI-6 (no version claimed)
    ATAATAPI6NoVersionClaimed = 0x15E0,
    /// ATA/ATAPI-6 ANSI INCITS 361-2002
    AtaAtapi6AnsiIncits3612002 = 0x15FD,
    /// ATA/ATAPI-7 (no version claimed)
    ATAATAPI7NoVersionClaimed = 0x1600,
    /// ATA/ATAPI-7 T13/1532-D revision 3
    ATAATAPI7T131532DRevision3 = 0x1602,
    /// ATA/ATAPI-7 ANSI INCITS 397-2005
    AtaAtapi7AnsiIncits3972005 = 0x161C,
    /// ATA/ATAPI-7 ISO/IEC 24739
    AtaAtapi7IsoIec24739 = 0x161E,
    /// ATA/ATAPI-8 ATA-AAM Architecture model (no version claimed)
    ATAATAPI8ATAAAMArchitectureModelNoVersionClaimed = 0x1620,
    /// ATA/ATAPI-8 ATA-PT Parallel transport (no version claimed)
    ATAATAPI8ATAPTParallelTransportNoVersionClaimed = 0x1621,
    /// ATA/ATAPI-8 ATA-AST Serial transport (no version claimed)
    ATAATAPI8ATAASTSerialTransportNoVersionClaimed = 0x1622,
    /// ATA/ATAPI-8 ATA-ACS ATA/ATAPI command set (no version claimed)
    ATAATAPI8ATAACSATAATAPICommandSetNoVersionClaimed = 0x1623,
    /// ATA/ATAPI-8 ATA-AAM ANSI INCITS 451-2008
    AtaAtapi8AtaAamAnsiIncits4512008 = 0x1628,
    /// ATA/ATAPI-8 ATA8-ACS ANSI INCITS 452-2009 w/ Amendment 1
    ATAATAPI8ATA8ACSANSIINCITS4522009WAmendment1 = 0x162A,
    /// Universal Serial Bus Specification, Revision 1.1
    UniversalSerialBusSpecificationRevision11 = 0x1728,
    /// Universal Serial Bus Specification, Revision 2.0
    UniversalSerialBusSpecificationRevision20 = 0x1729,
    /// USB Mass Storage Class Bulk-Only Transport, Revision 1.0
    USBMassStorageClassBulkOnlyTransportRevision10 = 0x1730,
    /// UAS (no version claimed)
    UASNoVersionClaimed = 0x1740,
    /// UAS T10/2095-D revision 02
    UAST102095DRevision02 = 0x1743,
    /// UAS T10/2095-D revision 04
    UAST102095DRevision04 = 0x1747,
    /// UAS ANSI INCITS 471-2010
    UasAnsiIncits4712010 = 0x1748,
    /// UAS ISO/IEC 14776-251:2014
    UasIsoIec147762512014 = 0x1749,
    /// ACS-2 (no version claimed)
    ACS2NoVersionClaimed = 0x1761,
    /// ACS-2 ANSI INCITS 482-2013
    Acs2AnsiIncits4822013 = 0x1762,
    /// ACS-3 INCITS 522-2014
    Acs3Incits5222014 = 0x1765,
    /// ACS-4 INCITS 529-2018
    Acs4Incits5292018 = 0x1767,
    /// UAS-2 (no version claimed)
    UAS2NoVersionClaimed = 0x1780,
    /// SAT (no version claimed)
    SATNoVersionClaimed = 0x1EA0,
    /// SAT T10/1711-D rev 8
    SATT101711DRev8 = 0x1EA7,
    /// SAT T10/1711-D rev 9
    SATT101711DRev9 = 0x1EAB,
    /// SAT ANSI INCITS 431-2007
    SatAnsiIncits4312007 = 0x1EAD,
    /// SAT-2 (no version claimed)
    SAT2NoVersionClaimed = 0x1EC0,
    /// SAT-2 T10/1826-D revision 06
    SAT2T101826DRevision06 = 0x1EC4,
    /// SAT-2 T10/1826-D revision 09
    SAT2T101826DRevision09 = 0x1EC8,
    /// SAT-2 ANSI INCITS 465-2010
    Sat2AnsiIncits4652010 = 0x1ECA,
    /// SAT-3 (no version claimed)
    SAT3NoVersionClaimed = 0x1EE0,
    /// SAT-3 T10/BSR INCITS 517 revision 4
    SAT3T10BSRINCITS517Revision4 = 0x1EE2,
    /// SAT-3 T10/BSR INCITS 517 revision 7
    SAT3T10BSRINCITS517Revision7 = 0x1EE4,
    /// SAT-3 ANSI INCITS 517-2015
    Sat3AnsiIncits5172015 = 0x1EE8,
    /// SAT-4 (no version claimed)
    SAT4NoVersionClaimed = 0x1F00,
    /// SAT-4 T10/BSR INCITS 491 revision 5
    SAT4T10BSRINCITS491Revision5 = 0x1F02,
    /// SAT-4 T10/BSR INCITS 491 revision 6
    SAT4T10BSRINCITS491Revision6 = 0x1F04,
    /// SPL (no version claimed)
    SPLNoVersionClaimed = 0x20A0,
    /// SPL T10/2124-D revision 6a
    SPLT102124DRevision6a = 0x20A3,
    /// SPL T10/2124-D revision 7
    SPLT102124DRevision7 = 0x20A5,
    /// SPL ANSI INCITS 476-2011
    SplAnsiIncits4762011 = 0x20A7,
    /// SPL ANSI INCITS 476-2011 + SPL AM1 INCITS 476/AM1 2012
    SplAnsiIncits4762011SplAm1Incits476Am12012 = 0x20A8,
    /// SPL ISO/IEC 14776-261:2012
    SplIsoIec147762612012 = 0x20AA,
    /// SPL-2 (no version claimed)
    SPL2NoVersionClaimed = 0x20C0,
    /// SPL-2 T10/BSR INCITS 505 revision 4
    SPL2T10BSRINCITS505Revision4 = 0x20C2,
    /// SPL-2 T10/BSR INCITS 505 revision 5
    SPL2T10BSRINCITS505Revision5 = 0x20C4,
    /// SPL-2 ANSI INCITS 505-2013
    Spl2AnsiIncits5052013 = 0x20C8,
    /// SPL-3 (no version claimed)
    SPL3NoVersionClaimed = 0x20E0,
    /// SPL-3 T10/BSR INCITS 492 revision 6
    SPL3T10BSRINCITS492Revision6 = 0x20E4,
    /// SPL-3 T10/BSR INCITS 492 revision 7
    SPL3T10BSRINCITS492Revision7 = 0x20E6,
    /// SPL-3 ANSI INCITS 492-2015
    Spl3AnsiIncits4922015 = 0x20E8,
    /// SPL-4 (no version claimed)
    SPL4NoVersionClaimed = 0x2100,
    /// SPL-4 T10/BSR INCITS 538 revision 08a
    SPL4T10BSRINCITS538Revision08a = 0x2102,
    /// SPL-4 T10/BSR INCITS 538 revision 10
    SPL4T10BSRINCITS538Revision10 = 0x2104,
    /// SPL-4 T10/BSR INCITS 538 revision 11
    SPL4T10BSRINCITS538Revision11 = 0x2105,
    /// SPL-5 (no version claimed)
    SPL5NoVersionClaimed = 0x2120,
    /// SOP (no version claimed)
    SOPNoVersionClaimed = 0x21E0,
    /// SOP T10/BSR INCITS 489 revision 4
    SOPT10BSRINCITS489Revision4 = 0x21E4,
    /// SOP T10/BSR INCITS 489 revision 5
    SOPT10BSRINCITS489Revision5 = 0x21E6,
    /// SOP ANSI INCITS 489-2014
    SopAnsiIncits4892014 = 0x21E8,
    /// PQI (no version claimed)
    PQINoVersionClaimed = 0x2200,
    /// PQI T10/BSR INCITS 490 revision 6
    PQIT10BSRINCITS490Revision6 = 0x2204,
    /// PQI T10/BSR INCITS 490 revision 7
    PQIT10BSRINCITS490Revision7 = 0x2206,
    /// PQI ANSI INCITS 490-2014
    PqiAnsiIncits4902014 = 0x2208,
    /// SOP-2 (no draft published)
    SOP2NoDraftPublished = 0x2220,
    /// PQI-2 (no version claimed)
    PQI2NoVersionClaimed = 0x2240,
    /// PQI-2 T10/BSR INCITS 507 revision 01
    PQI2T10BSRINCITS507Revision01 = 0x2242,
    /// PQI-2 PQI-2 ANSI INCITS 507-2016
    Pqi2Pqi2AnsiIncits5072016 = 0x2244,
    /// IEEE 1667 (no version claimed)
    IEEE1667NoVersionClaimed = 0xFFC0,
    /// IEEE 1667-2006
    Ieee16672006 = 0xFFC1,
    /// IEEE 1667-2009
    Ieee16672009 = 0xFFC2,
    /// IEEE 1667-2015
    Ieee16672015 = 0xFFC3,
    /// IEEE 1667-2018
    Ieee16672018 = 0xFFC4,
}
impl Default for VersionDescriptor {
    fn default() -> Self { VersionDescriptor::None }
}
