# Contributing

Contributions are welcome!

Contributions can be bug reports, feature requests, testing and documentation
in addition to code. Please see the github guide on
[Collaborating on projects using issues and pull requests](https://help.github.com/categories/collaborating-on-projects-using-issues-and-pull-requests/) for details.

Contributions to this project are accepted on an
["inbound=outbound"](https://opensource.com/law/11/7/trouble-harmony-part-1) basis.
That means that you agree that your contributions are made under the
same license as the license for this project (found in this directory in the [LICENSE](LICENSE) file).

To make this understanding explicit -- and for you to assert
that you have the right to make the contribution -- commits must be
signed off indicating acceptance of the
[Developer Certificate of Origin 1.1](https://developercertificate.org/).
A nice explanation of the DCO has been provided by
[Karl Fogel](https://www.red-bean.com/kfogel/)
in his excellent book [Producing Open Source Software](https://producingoss.com/en/contributor-agreements.html#developer-certificate-of-origin).
An explanation of the "sign-off" procedure is given by
[Linus Torvalds](https://en.wikipedia.org/wiki/Linus_Torvalds) in [Linux](https://github.com/torvalds/linux/blob/master/Documentation/process/submitting-patches.rst#11-sign-your-work---the-developers-certificate-of-origin).

## Code of Conduct

Please note that this project is released with a
[Contributor Code of Conduct](CODE_OF_CONDUCT.md)
(adopted from the [Contributor Covenant v2.0](https://www.contributor-covenant.org/)).

By participating in this project you agree to abide by the
[Contributor Code of Conduct](CODE_OF_CONDUCT.md)
(please read the full text so that you can understand what actions will and
will not be tolerated).

## Contribution Workflow

This is an overview of the contribution workflow:

 * Fork the repository on Github
 * Create a topic branch from where you want to base your work (usually from the master branch)
 * Check the formatting rules from existing code (no trailing whitespace, mostly default indentation)
 * Ensure any new code is well-tested, and if possible, any issue fixed is covered by one or more new tests
 * Make commits to your branch using the following guidelines:
   * Start with a subject line (beginning with a capital, ending without a period, no more than 50 characters)
   * The second line should be blank
   * The body starts on the third line and may
     [reference existing issues](https://help.github.com/en/github/managing-your-work-on-github/closing-issues-using-keywords)
     (e.g. `Closes #1`)
   * Use the imperative mood in the subject line: "Add x", "Fix y", "Support z", "Remove x"
   * Wrap the body at 72 characters
   * Use the body to explain what and why vs. how
   * Finish the commit message with the sign off: `Signed-off-by: Your Name <me@e.mail>`
 * Push your code to your fork of the repository
 * Make a Pull Request

## Emacs

Are you an Emacs user? If so you can use `magit-commit-popup`
(from [magit](https://magit.vc/)) to add these
commit options for you:
 * **-s** Add Signed-off-by line (--signoff)
 * **=S** Sign using gpg (--gpg-sign="0xCAFED00D") -- *this is extra credit*

Did you know that Emacs can delete trailing whitespace without
you *every having to think about it*? Just add this to your
Emacs configuration:

````
(add-hook 'before-save-hook 'delete-trailing-whitespace)
````

## Verified commits with GPG

Verifying commits is not essential for contributions to this project,
but for those motivated to add additional security it is important
to know that Github now [supports GPG signature verification](https://github.com/blog/2144-gpg-signature-verification).

Using GPG is a complex subject. Here are some pointers for further information:
 * [OpenPGP Best Practices](https://help.riseup.net/en/security/message-security/openpgp/best-practices)
 * [Debian Keysigning HOWTO](https://wiki.debian.org/Keysigning)
 * If you create a new key please ensure that the RSA key length is at least
   4096 bits and configured for [SHA-2](https://www.debian-administration.org/users/dkg/weblog/48)
 * If you use **caff** (from [signing-party](https://packages.debian.org/sid/signing-party)) to sign and distrubute signatures please verify that the *separate* [caff configuration is accurate](https://github.com/tmarble/kspsig).

You can export your GPG public key to a file for use with Github as follows
(assuming your GPG key id is 0xCAFED00D):

````
gpg --output 0xCAFED00D.asc --armor --export-options export-clean,export-minimal --export 0xCAFED00D
````

Simply paste the contents of that file in the **SSH and GPG keys** section of your [Github settings](https://help.github.com/articles/adding-a-new-gpg-key-to-your-github-account/).
