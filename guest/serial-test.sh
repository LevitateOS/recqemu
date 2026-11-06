# Serial test protocol (recqemu `serial::protocol`): installed as a
# profile.d script. On the serial console only, it turns terminal echo off,
# empties the prompt and announces that the shell takes commands, so the
# host reads command output without echoes or prompts mixed in. POSIX sh
# (bash and busybox ash).
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac
[ "$(tty 2>/dev/null)" = "/dev/ttyS0" ] || return 0

stty -echo 2>/dev/null
PS1=''
PS2=''
echo "@SHELL_READY_MARKER@"
