;;; SPDX-FileCopyrightText: © 2023 Foundation Devices <hello@foundationdevices.com>
;;; SPDX-License-Identifier: GPL-3.0-or-later
;;;
;;; Commentary:
;;;
;;; This file describes which exact version of GNU Guix and additional
;;; channels are used.  Used to perform reproducible builds regardless of the
;;; current version of the user's GNU Guix version.
;;;
;;; To update this file:
;;;
;;; guix describe -f channels > channels.scm
;;;
;;; Example, using time machine to build the packages listed in the guix.scm
;;; file:
;;;
;;; guix time-machine --channels=channels.scm -- build -f guix.scm
;;;

(list (channel
        (name 'guix)
        (url "https://git.savannah.gnu.org/git/guix.git")
        (branch "master")
        (commit
          "b94cbbbce70f59b795526a0ed305facf041e6faa")
        (introduction
          (make-channel-introduction
            "9edb3f66fd807b096b48283debdcddccfea34bad"
            (openpgp-fingerprint
              "BBB0 2DDF 2CEA F6A8 0D1D  E643 A2A0 6DF2 A33A 54FA")))))
