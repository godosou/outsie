#include <errno.h>
#include <libproc.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int repose_listener_is_accepting(int socket_fd) {
    if (socket_fd < 0) {
        return -1;
    }

    int accepting = 0;
    socklen_t accepting_length = (socklen_t)sizeof(accepting);
    if (getsockopt(socket_fd,
                   SOL_SOCKET,
                   SO_ACCEPTCONN,
                   &accepting,
                   &accepting_length) == 0) {
        if (accepting_length != (socklen_t)sizeof(accepting)) {
            return -2;
        }
        return accepting != 0 ? 1 : 0;
    }
    if (errno != ENOPROTOOPT) {
        return -3;
    }

    /*
     * Darwin 23 exposes SO_ACCEPTCONN in the SDK but returns ENOPROTOOPT for
     * AF_UNIX. The kernel's documented proc socket snapshot contains the same
     * SO_ACCEPTCONN bit, so use it only for that platform-specific failure.
     */
    struct socket_fdinfo info;
    memset(&info, 0, sizeof(info));
    int copied = proc_pidfdinfo(getpid(),
                                socket_fd,
                                PROC_PIDFDSOCKETINFO,
                                &info,
                                (int)sizeof(info));
    if (copied != (int)sizeof(info)) {
        return -4;
    }
    return (info.psi.soi_options & SO_ACCEPTCONN) != 0 ? 1 : 0;
}
