param (
    [Boolean]$submodules = $true,
    [switch]$debug = $false,
    [switch]$clean = $false
)

# Note that mingw64 should be installed.  Ensure you do the following
# inside the shell in order to have the compiler installed:
# $ C:\tools\msys64\mingw64 bash -c "pacman -S mingw64/mingw-w64-x86_64-gcc"

# Add mingw64 to the PATH.
# This is required due to bugs in how gcc calls cc1.exe in a separate directory,
# which needs to find libgmp-10.dll and libwinpthread-1.dll, but doesn't know
# how to locate it.
$env:PATH += ";C:\tools\msys64\mingw64\bin\"

# Add msbuild.exe to the path
$env:PATH += ";C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\"

$target = "Release"
if ($debug) {
    $target = "Debug"
}

# We should only do this if "-s" is passed, or if submodules
# are not initialized
if ($submodules) {
    git submodule update --init --recursive
}

# renode-resources is a repository of binaries that are infrequently
# updated, and therefore isn't a submodule.
git -C lib clone https://github.com/renode/renode-resources.git resources

# Update references to Xwt
(Get-Content .\lib\termsharp\TermSharp.csproj)|
    ForEach-Object {$_ -replace """xwt\\Xwt\\Xwt.csproj", """..\xwt\Xwt\Xwt.csproj"} |
    Set-Content .\lib\termsharp\TermSharp.csproj

# Build CCTask, which is used to run subtasks
MSBuild /p:Configuration=Release /p:Platform="Any CPU" .\lib\cctask\CCTask.sln

# Copy the properties file, which contains various build environment settings
if (-not (Test-path output\properties.csproj)) {
    New-Item -Path output -ItemType Directory -Force
    Copy-Item src\Infrastructure\src\Emulator\Cores\windows-properties.csproj output\properties.csproj
}

# Run the "clean" task if the user requested
if ($clean) {
    MSBuild /p:Configuration=Release /p:Platform="Any CPU" /t:Clean Renode-Windows.sln
    MSBuild /p:Configuration=Debug /p:Platform="Any CPU" /t:Clean Renode-Windows.sln
    return
}

MSBuild /p:Configuration=$target /p:Platform="Any CPU" Renode-Windows.sln /p:GenerateFullPaths=true

# copy llvm library
Copy-Item src/Infrastructure/src/Emulator/LLVMDisassembler/bin/$target/libLLVM.* output/bin/$target