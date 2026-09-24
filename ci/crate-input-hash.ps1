# #904 (part of #901): the input hash a proof shard is keyed by.
#
# The gate today proves a HEAD. Main moves, the head moves, and the proof dies with it even when
# not one byte the run compiled has changed. A shard keyed by the INPUTS survives both, without
# weakening what proof means -- provided the key really covers everything that lands.
#
# That proviso is the whole risk, and #904 names it: "an input the hash omits (env, an undeclared
# file read by build.rs, a feature unified from elsewhere, the toolchain) lets a stale green replay
# against different bytes -- silent proof corruption." A hash that is wrong in the SAFE direction
# (too many inputs) costs a rebuild. A hash that is wrong in the UNSAFE direction certifies bytes
# nobody compiled, and does it silently. So every choice below is made toward the safe side, and
# the ones that are not obvious say why.
#
# WHAT THIS FILE IS NOT. It writes no shard, reads no shard, and merge-proof (retired 2026-09-24)
# never called it. Shard storage belongs to #902's runner and per-crate scope to #903, both in
# flight in other lanes; putting a store here would give one file two owners. This is the key those
# two need, and it is the half that has to be right BEFORE anything reuses a proof. Composition is
# separated from gathering for the same reason the rest of ci/ separates them: the composition is a
# pure function, so its properties can be measured without a repository, a toolchain, or a slot.

Set-StrictMode -Version Latest

# The field separator inside a hash preimage. A record separator (0x1E) cannot occur in a git
# object id, a crate name, a feature name, or a path this repository can produce -- but "cannot
# occur" is an argument, and an argument is not a guard, so every field is ALSO length-prefixed
# below. See New-HashPreimage.
$script:FieldSeparator = [char]0x1E

function Get-Sha256Hex {
    <#
      .SYNOPSIS
        Lowercase hex SHA-256 of a string, encoded UTF-8 without a BOM.

      .DESCRIPTION
        UTF8Encoding($false) rather than [Text.Encoding]::UTF8, whose GetBytes does not emit a
        preamble but whose name invites the assumption that it might; being explicit costs one
        argument and removes the question. The digest is the identity of the preimage and nothing
        else -- callers that need meaning ask New-HashPreimage what went in.
    #>
    param([Parameter(Mandatory)][AllowEmptyString()][string] $Text)

    $encoding = New-Object System.Text.UTF8Encoding($false)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $sha.ComputeHash($encoding.GetBytes($Text))
    } finally {
        $sha.Dispose()
    }
    return -join ($bytes | ForEach-Object { $_.ToString('x2') })
}

function Sort-Ordinal {
    <#
      .SYNOPSIS
        Sorts strings by ORDINAL comparison, which `Sort-Object` does not do.

      .DESCRIPTION
        `Sort-Object -CaseSensitive` is still CULTURE-sensitive: it orders through the current
        culture's collation, where `-` and `_` sort differently under different cultures and some
        cultures ignore punctuation entirely. A shard key whose field order depended on the
        machine's locale would be written under one key on one lane's box and looked up under
        another on the next -- a miss that costs a rebuild rather than a false green, so it is the
        safe direction, but it would make the cache silently useless across a fleet and the cause
        would be invisible.

        `[StringComparer]::Ordinal` compares UTF-16 code units. Nothing about it moves.
    #>
    param([AllowEmptyCollection()][AllowNull()][string[]] $Values)

    $list = New-Object 'System.Collections.Generic.List[string]'
    foreach ($value in @($Values)) { if ($null -ne $value) { $list.Add($value) } }
    $list.Sort([StringComparer]::Ordinal)
    return , $list.ToArray()
}

function Join-Elements {
    <#
      .SYNOPSIS
        Renders a SET of strings unambiguously, as `<byte length>:<element>` joined by commas.

      .DESCRIPTION
        THIS is the length prefix that carries weight, and finding that out cost a sabotage that
        reddened nothing.

        The outer field join was written length-prefixed first, on the argument that a separator is
        ambiguous when a value can contain it. Cutting that prefix out and running the suite turned
        nothing red -- correctly, because with a FIXED number of fields and FIXED distinct field
        names (`crate=`, `tree=`, ...) the outer rendering is already injective: absorbing a
        separator into a value would have to make the next field's name appear where a different
        name is required, and the arity is not free to change. The prefix there is belt and braces
        against a future edit that makes the field list variable, and the comment used to claim it
        was load-bearing. It was not.

        One level in, the ambiguity is REAL and was live in this file: the sets were joined with a
        comma, so a single element `proto/a,b.proto` and the pair `proto/a` + `b.proto` produced
        BYTE-IDENTICAL preimages and therefore one cache key. Two different input sets, one key --
        the exact silent-proof-corruption shape #904's threat assessment names, sitting inside the
        function meant to prevent it. Feature names cannot contain a comma, but dependency keys and
        build-script input PATHS can, and "the inputs cannot spell it" is an argument, not a guard.

        `<len>:<elem>` is injective for any element whatever. It also separates the empty SET from
        the set holding one empty string, which `-join ','` spells identically.
    #>
    param([AllowEmptyCollection()][AllowNull()][string[]] $Values)

    $encoding = New-Object System.Text.UTF8Encoding($false)
    $parts = foreach ($value in @($Values)) {
        "$($encoding.GetByteCount($value)):$value"
    }
    return (@($parts) -join ',')
}

function New-HashPreimage {
    <#
      .SYNOPSIS
        The exact bytes that get hashed, as a string, so a caller can see them and a cell can
        assert on them without inverting a digest.

      .DESCRIPTION
        Length-prefixed at BOTH levels -- see Join-Elements for which of the two is load-bearing
        and which is belt and braces, and for the collision that was live here until a sabotage
        failed to redden and sent me looking.

        ORDER IS NORMALISED for the two sets -- features and dependency hashes. Cargo does not
        promise an enumeration order and neither does a filesystem walk; a key that changed when
        the same inputs arrived in a different order would miss its own shard and quietly rebuild
        forever, which is the safe direction but still a defect. Sorted with Ordinal, not the
        ambient culture: `[string]::Compare` under a non-invariant culture orders `-` and `_`
        differently, so a shard written on one machine would not be found on another.
    #>
    param(
        [Parameter(Mandatory)][string] $Crate,
        [Parameter(Mandatory)][string] $TreeObject,
        [Parameter(Mandatory)][AllowEmptyString()][string] $LockSlice,
        [AllowEmptyString()][string] $WorkspaceManifest,
        [Parameter(Mandatory)][string] $ToolchainId,
        [AllowEmptyCollection()][string[]] $Features = @(),
        [AllowEmptyCollection()][string[]] $DependencyHashes = @(),
        [AllowEmptyCollection()][string[]] $BuildScriptInputs = @()
    )

    # NOT `[Parameter(Mandatory)]`, and the difference is measured rather than stylistic.
    # Under the gate's own invocation (`powershell -NoProfile -ExecutionPolicy Bypass -File`,
    # with no `-NonInteractive`) a missing Mandatory parameter PROMPTS: measured rc=124 after
    # 40s, output cut off at the first line. A caller that forgets this argument would hang the
    # gate rather than fail it, and a hang has no colour. Adding this throw BESIDE Mandatory
    # does not help either: the binder raises ParameterBindingException before the body runs,
    # so the message below would be unreachable and a cell asserting it would be vacuous
    # (measured: 'Cannot process command because of one or more missing mandatory parameters').
    # So it is this, alone: it fires in EVERY host, it carries a message that names the input,
    # and a cell can reach it. (K's line on #923, X's risk on #924.)
    if (-not $PSBoundParameters.ContainsKey('WorkspaceManifest')) {
        throw 'WorkspaceManifest was not supplied. It is an input to the key, not an option: omitting it would hash every root manifest as empty and give two different workspaces one key. Pass the blob id, or pass '''' to say on the record that there is none.'
    }

    $features = Sort-Ordinal -Values $Features
    $dependencies = Sort-Ordinal -Values $DependencyHashes
    $buildInputs = Sort-Ordinal -Values $BuildScriptInputs

    $fields = @(
        "crate=$Crate",
        "tree=$TreeObject",
        "lock=$LockSlice",
        "manifest=$WorkspaceManifest",
        "toolchain=$ToolchainId",
        "features=$(Join-Elements -Values $features)",
        "deps=$(Join-Elements -Values $dependencies)",
        "buildinputs=$(Join-Elements -Values $buildInputs)"
    )

    # `<byte length>:<field>` per field. The length is of the UTF-8 bytes, not of the .NET string:
    # a character outside the BMP is one char pair and four bytes, and the two counts disagree.
    $encoding = New-Object System.Text.UTF8Encoding($false)
    $prefixed = foreach ($field in $fields) {
        "$($encoding.GetByteCount($field)):$field"
    }
    return (@($prefixed) -join $script:FieldSeparator)
}

function Get-CrateInputHash {
    <#
      .SYNOPSIS
        The content-addressed key for one crate's build inputs, namespaced by toolchain generation.

      .DESCRIPTION
        Returns `<generation>-<digest>` where the generation is the first 12 hex of the toolchain
        id's own digest. #904 asks for that namespacing so a Rust bump starts a fresh generation
        instead of reusing shards across compilers -- and putting it in the RETURNED STRING rather
        than only inside the digest is what makes a stale key legible: a human reading two keys
        can see they belong to different toolchains, instead of seeing two opaque hashes that
        merely differ.

        The toolchain id is folded in twice on purpose (as the generation prefix and as a field of
        the preimage). Belt and braces cost nothing here, and the alternative -- prefix only --
        would let two different toolchains that happen to share a 12-hex prefix produce keys whose
        SUFFIXES are equal for equal sources, which is a needless coincidence to leave lying
        around in a cache key.
    #>
    param(
        [Parameter(Mandatory)][string] $Crate,
        [Parameter(Mandatory)][string] $TreeObject,
        [Parameter(Mandatory)][AllowEmptyString()][string] $LockSlice,
        [AllowEmptyString()][string] $WorkspaceManifest,
        [Parameter(Mandatory)][string] $ToolchainId,
        [AllowEmptyCollection()][string[]] $Features = @(),
        [AllowEmptyCollection()][string[]] $DependencyHashes = @(),
        [AllowEmptyCollection()][string[]] $BuildScriptInputs = @()
    )

    # NOT `[Parameter(Mandatory)]`, and the difference is measured rather than stylistic.
    # Under the gate's own invocation (`powershell -NoProfile -ExecutionPolicy Bypass -File`,
    # with no `-NonInteractive`) a missing Mandatory parameter PROMPTS: measured rc=124 after
    # 40s, output cut off at the first line. A caller that forgets this argument would hang the
    # gate rather than fail it, and a hang has no colour. Adding this throw BESIDE Mandatory
    # does not help either: the binder raises ParameterBindingException before the body runs,
    # so the message below would be unreachable and a cell asserting it would be vacuous
    # (measured: 'Cannot process command because of one or more missing mandatory parameters').
    # So it is this, alone: it fires in EVERY host, it carries a message that names the input,
    # and a cell can reach it. (K's line on #923, X's risk on #924.)
    if (-not $PSBoundParameters.ContainsKey('WorkspaceManifest')) {
        throw 'WorkspaceManifest was not supplied. It is an input to the key, not an option: omitting it would hash every root manifest as empty and give two different workspaces one key. Pass the blob id, or pass '''' to say on the record that there is none.'
    }

    $preimage = New-HashPreimage -Crate $Crate -TreeObject $TreeObject -LockSlice $LockSlice `
        -WorkspaceManifest $WorkspaceManifest -ToolchainId $ToolchainId -Features $Features `
        -DependencyHashes $DependencyHashes -BuildScriptInputs $BuildScriptInputs
    $generation = (Get-Sha256Hex -Text $ToolchainId).Substring(0, 12)
    return "$generation-$(Get-Sha256Hex -Text $preimage)"
}

function Get-CrateTreeObject {
    <#
      .SYNOPSIS
        The git tree object id of a crate's whole directory at a revision.

      .DESCRIPTION
        THE WHOLE DIRECTORY, not a file list, and that is the guard against the omission this
        issue's threat assessment names. A hash over `src/**/*.rs` plus `Cargo.toml` misses the
        file nobody thought of -- a `build.rs` include, a `.proto`, a fixture read at compile time,
        a `.cargo/config.toml` dropped in beside it. The tree object covers every path under the
        directory as git records it, so an unhashed file is not a category that exists.

        It reads from the OBJECT DATABASE at a revision, not from the working tree: the answer is
        about the bytes a merge would land, not about whatever is currently checked out with
        somebody's half-finished edit in it. That also makes it usable on a bare fetch of a PR head
        with nothing checked out at all.

        `--full-tree` is not applicable to `rev-parse <rev>:<path>` -- but the reason it exists for
        `ls-tree` applies here too, and the answer to it is the same: the path is given relative to
        the repository ROOT, and `-C $RepositoryRoot` fixes what "root" means rather than
        inheriting the caller's current directory.
    #>
    param(
        [Parameter(Mandatory)][string] $RepositoryRoot,
        [Parameter(Mandatory)][string] $Revision,
        [Parameter(Mandatory)][string] $CratePath
    )

    $normalised = $CratePath -replace '\\', '/'
    $normalised = $normalised.Trim('/')
    $global:LASTEXITCODE = 0
    $object = & git -C $RepositoryRoot rev-parse "$($Revision):$normalised" 2>&1
    if ($LASTEXITCODE -ne 0) {
        # The path, the revision AND what git said. An error that names only "could not read the
        # crate" sends the reader to the wrong subsystem; more than one instrument in this
        # repository has failed without naming what it saw.
        throw "cannot read the tree object for '$normalised' at '$Revision' in '$RepositoryRoot': $object"
    }
    return ([string]$object).Trim()
}

function Get-WorkspaceManifestBlob {
    <#
      .SYNOPSIS
        The blob id of the workspace ROOT manifest at a revision — an input that is in no crate's tree.

      .DESCRIPTION
        Found by ISSUES 4 reviewing this file, and it is the unsafe direction, so it is worth stating
        in full rather than as a parameter name.

        `Get-CrateTreeObject` covers everything under a crate's directory, which is what makes "a file
        nobody listed" not a category. The build PROFILES are not under any crate:

            Cargo.toml:74   [profile.release]  lto = "thin"  codegen-units = 1
            Cargo.toml:78   [profile.test]     debug = 1

        Measured: `[profile]` blocks in core/schema, core/events, adapters/tool-host, apps/cli = 0, 0,
        0, 0. Cargo forbids them in members; they live in the root and nowhere else. Change `lto` to
        `"fat"` and EVERY crate compiles to something different while every crate's key stays
        byte-identical — the tree object cannot see it (different directory) and the lock slice cannot
        carry it (`Cargo.lock` records resolved versions, not profiles). Two trees that build
        differently, one key.

        The blob id is the cheapest correct handle: it moves whenever ANY byte of that file moves, so
        it covers the profiles without this code having to know what a profile is — the same reason
        the crate hash is a tree object rather than a file list.

        MANDATORY, and it is the only decision in this function where the ATTRIBUTE is the guard
        (D and ISSUES 4, reviewing #921). Every other input is `[Parameter(Mandatory)]`; this one
        shipped as `= ''` and was the only optional one -- the UNSAFE input, optional. A caller who
        simply forgets it gets the empty-manifest key, byte-identical for every possible root
        manifest, which is exactly the collision this field exists to close, re-entering through a
        parameter default. Measured before the fix: omitting it and passing `''` produced the same
        key, while two different manifests produced different ones -- so the field worked and the
        default silently disarmed it.

        `[AllowEmptyString()]` stays beside `Mandatory`, which is the idiom already one line above
        on `$LockSlice`: *it may legitimately be empty, but you have to say so.* A caller with no
        workspace manifest passes `''` and that is a decision on the record; a caller who forgets
        gets an error instead of a wrong key.

        WHAT THE KEY STILL DOES NOT SEE, stated because a list of covered inputs invites the reader to
        assume the rest is covered too:
          - `RUSTFLAGS` / `CARGO_ENCODED_RUSTFLAGS` in the environment;
          - an explicit `--target` (`rustc -Vv` reports the HOST triple, which does not move when you
            cross-compile);
          - `CARGO_PROFILE_*` overrides in the environment;
          - a file a `build.rs` reads without declaring it — that is acceptance 6's hermeticity linter,
            and until it lands `BuildScriptInputs` is only as good as its caller.
        ISSUES 4 measured that the gate sets none of the first three today (`grep RUSTFLAGS|--release|
        --target|CARGO_PROFILE|CARGO_BUILD` over `ci/gate.ps1` and `ci/postgres.ps1` returns nothing),
        so they are omissions the current caller cannot reach; the profile in `Cargo.toml` was
        different because it is checked in and an ordinary commit can change it.
    #>
    param(
        [Parameter(Mandatory)][string] $RepositoryRoot,
        [Parameter(Mandatory)][string] $Revision,
        [string] $ManifestPath = 'Cargo.toml'
    )

    $global:LASTEXITCODE = 0
    $blob = & git -C $RepositoryRoot rev-parse "$($Revision):$ManifestPath" 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "cannot read the workspace manifest blob for '$ManifestPath' at '$Revision' in '$RepositoryRoot': $blob"
    }
    return ([string]$blob).Trim()
}

function Get-ToolchainId {
    <#
      .SYNOPSIS
        The compiler identity a generation is namespaced by: `rustc -Vv` plus the host triple.

      .DESCRIPTION
        The FULL verbose output, not `rustc --version`. The short form omits the commit hash and
        the LLVM version, and two nightlies a week apart can print the same short string while
        generating different code. This is a cache key: the safe direction is to include more.

        `-ExpectedFailureIsFatal` is deliberately absent. A caller that cannot run rustc gets a
        throw, because the alternative -- returning a placeholder and carrying on -- is an
        infallible fallback, and an infallible fallback turns a gap into drift: every crate on
        that machine would share one generation and reuse each other's shards.
    #>
    param([string] $RustcPath = 'rustc')

    $global:LASTEXITCODE = 0
    $verbose = & $RustcPath -Vv 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "cannot read the toolchain id: '$RustcPath -Vv' exited $LASTEXITCODE ($verbose)"
    }
    return ((@($verbose) -join "`n") -replace "`r", '').Trim()
}
