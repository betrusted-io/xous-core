;;; Guix development environment for baochip
;;;
;;; Usage:
;;;   guix shell --pure --development --file=guix.scm
;;;   cargo xtask dabao helloworld

(use-modules (bao))

;; Re-export the dev shell from bao.scm
xous-dev-shell
