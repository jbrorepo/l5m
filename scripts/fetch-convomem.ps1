param(
    [string]$OutDir = "data\ConvoMem",
    [switch]$Full,
    [string]$Token = $env:HF_TOKEN,
    [int]$MaxWorkers = 1
)

$ErrorActionPreference = "Stop"
$env:PYTHONIOENCODING = "utf-8"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if (Get-Command huggingface-cli -ErrorAction SilentlyContinue) {
    $args = @(
        "download",
        "Salesforce/ConvoMem",
        "--repo-type", "dataset",
        "--local-dir", $OutDir,
        "--max-workers", "$MaxWorkers"
    )
    if ($Token) {
        $args += @("--token", $Token)
    }
    if (-not $Full) {
        $args += @(
            "--include",
            "core_benchmark/evidence_questions/user_evidence/**",
            "core_benchmark/evidence_questions/assistant_facts_evidence/**",
            "core_benchmark/evidence_questions/changing_evidence/**",
            "core_benchmark/evidence_questions/abstention_evidence/**",
            "core_benchmark/evidence_questions/preference_evidence/**",
            "core_benchmark/evidence_questions/implicit_connection_evidence/**"
        )
    }
    & huggingface-cli @args
    exit $LASTEXITCODE
}

if (Get-Command git -ErrorAction SilentlyContinue) {
    if (-not (Get-Command git-lfs -ErrorAction SilentlyContinue)) {
        throw "git is installed, but git-lfs is not available. Install Hugging Face CLI or git-lfs."
    }
    git lfs install
    git clone https://huggingface.co/datasets/Salesforce/ConvoMem $OutDir
    exit $LASTEXITCODE
}

throw "Install Hugging Face CLI or git-lfs to fetch ConvoMem."
