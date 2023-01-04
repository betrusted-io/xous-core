use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum TargetPortGroupSupport {
    //The logical unit does not support asymmetric logical unit access or supports a form of asymmetric access that is vendor specific. Neither the REPORT TARGET GROUPS nor the SET TARGET PORT GROUPS commands is supported.
    Unsupported = 0b00,
    //The logical unit supports only implicit asymmetric logical unit access (see 5.11.2.7). The logical unit is capable of changing target port asymmetric access states without a SET TARGET PORT GROUPS command. The REPORT TARGET PORT GROUPS command is supported and the SET TARGET PORT GROUPS command is not supported.
    Implicit = 0b01,
    //The logical unit supports only explicit asymmetric logical unit access (see 5.11.2.8). The logical unit only changes target port asymmetric access states as requested with the SET TARGET PORT GROUPS command. Both the REPORT TARGET PORT GROUPS command and the SET TARGET PORT GROUPS command are supported.
    Explicit = 0b10,
    //The logical unit supports both explicit and implicit asymmetric logical unit access. Both the REPORT TARGET PORT GROUPS command and the SET TARGET PORT GROUPS commands are supported.
    ImplicitAndExplicit = 0b11,
}
impl Default for TargetPortGroupSupport {
    fn default() -> Self {
        TargetPortGroupSupport::Unsupported
    }
}