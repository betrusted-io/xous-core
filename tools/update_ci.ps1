param (
    [string]$LOCALE="en"
)
Write-Output "This script will update Xous and stage gateware and EC firmware for self-provisioning. No secrets will be erased."
Write-Output "This script will use the bleeding edge CI build."
$title    = 'Update to bleeding edge CI'
$question = 'Are you sure you want to proceed?'
$choices  = '&Yes', '&No'

$decision = $Host.UI.PromptForChoice($title, $question, $choices, 1)
if ($decision -eq 0) {
    Write-Host 'confirmed'
} else {
    Exit
}

Invoke-WebRequest https://ci.betrusted.io/latest-ci/loader.bin -OutFile loader.bin
python usb_update.py -l loader.bin
Remove-Item loader.bin

Write-Output "waiting for device to reboot"
Start-Sleep 5

Invoke-WebRequest https://ci.betrusted.io/latest-ci/xous-$LOCALE.img -OutFile xous.img
python usb_update.py -k xous.img
Remove-Item xous.img

Write-Output "waiting for device to reboot"
Start-Sleep 5

Invoke-WebRequest https://ci.betrusted.io/latest-ci/soc_csr.bin -OutFile soc_csr.bin
python usb_update.py -s soc_csr.bin
Remove-Item soc_csr.bin

Write-Output "waiting for device to reboot"
Start-Sleep 5

Invoke-WebRequest https://ci.betrusted.io/latest-ci/ec_fw.bin -OutFile ec_fw.bin
python usb_update.py -e ec_fw.bin
Remove-Item ec_fw.bin

Write-Output "waiting for device to reboot"
Start-Sleep 5

Invoke-WebRequest https://ci.betrusted.io/latest-ci/wf200_fw.bin -OutFile wf200_fw.bin
python usb_update.py -w wf200_fw.bin
Remove-Item wf200_fw.bin

Write-Output "IMPORTANT: you must select 'Install gateware update' on the Precursor device to update the SoC."
Write-Output "IMPORTANT: you must also run 'ecup auto' to update the EC with the staged firmware objects."
