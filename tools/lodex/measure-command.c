/* Linux/OpenWrt benchmark launcher: raw kernel counters, no argv/env logging. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t measured_pid = -1;
static void forward_signal(int signal_number) {
    if (measured_pid > 0) (void)kill((pid_t)measured_pid, signal_number);
}
static double seconds(struct timeval value) {
    return (double)value.tv_sec + (double)value.tv_usec / 1000000.0;
}
static int calibration(void) {
    const size_t size = 32U * 1024U * 1024U;
    volatile unsigned char *memory = malloc(size);
    if (!memory) return 1;
    for (size_t offset = 0; offset < size; offset += 4096) memory[offset] = 1;
    FILE *status = fopen("/proc/self/status", "r");
    if (!status) { free((void *)memory); return 1; }
    char line[256];
    long high_water = -1;
    while (fgets(line, sizeof(line), status)) {
        if (sscanf(line, "VmHWM: %ld", &high_water) == 1) break;
    }
    fclose(status);
    struct rusage usage;
    if (getrusage(RUSAGE_SELF, &usage) || high_water < 0) {
        free((void *)memory); return 1;
    }
    printf("{\"proc_hwm_kib\":%ld,\"getrusage_maxrss_kib\":%ld}\n", high_water, usage.ru_maxrss);
    free((void *)memory);
    return ferror(stdout) ? 1 : 0;
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--calibrate")) return calibration();
    if (argc < 3) {
        fputs("usage: measure-command NEW_METRICS_FILE PROGRAM [ARGS]\n", stderr);
        return 2;
    }
    int descriptor = open(argv[1], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (descriptor < 0) { fputs("could not create new metrics file\n", stderr); return 2; }
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_handler = forward_signal;
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGTERM, &action, NULL) || sigaction(SIGINT, &action, NULL) || sigaction(SIGHUP, &action, NULL)) {
        close(descriptor); return 2;
    }
    sigset_t handled, previous_mask;
    sigemptyset(&handled);
    sigaddset(&handled, SIGTERM); sigaddset(&handled, SIGINT); sigaddset(&handled, SIGHUP);
    if (sigprocmask(SIG_BLOCK, &handled, &previous_mask)) { close(descriptor); return 2; }
    struct timespec start, finish;
    if (clock_gettime(CLOCK_MONOTONIC, &start)) { close(descriptor); return 2; }
    pid_t child = fork();
    if (child < 0) { close(descriptor); return 2; }
    if (child == 0) {
        signal(SIGTERM, SIG_DFL); signal(SIGINT, SIG_DFL); signal(SIGHUP, SIG_DFL);
        if (sigprocmask(SIG_SETMASK, &previous_mask, NULL)) _exit(126);
        execvp(argv[2], &argv[2]);
        _exit(errno == ENOENT ? 127 : 126);
    }
    measured_pid = child;
    if (sigprocmask(SIG_SETMASK, &previous_mask, NULL)) {
        kill(child, SIGKILL); waitpid(child, NULL, 0); close(descriptor); return 2;
    }
    /* Keep the exited child unreaped until forwarding is disabled, so a late
       signal cannot target a PID that the kernel has already reused. */
    siginfo_t exited;
    int observed;
    do { observed = waitid(P_PID, (id_t)child, &exited, WEXITED | WNOWAIT); }
    while (observed < 0 && errno == EINTR);
    measured_pid = -1;
    if (observed < 0) { kill(child, SIGKILL); waitpid(child, NULL, 0); close(descriptor); return 2; }
    int status;
    struct rusage usage;
    pid_t waited;
    do { waited = wait4(child, &status, 0, &usage); } while (waited < 0 && errno == EINTR);
    measured_pid = -1;
    if (waited != child || clock_gettime(CLOCK_MONOTONIC, &finish)) { close(descriptor); return 2; }
    double wall = (double)(finish.tv_sec - start.tv_sec) + (double)(finish.tv_nsec - start.tv_nsec) / 1000000000.0;
    int exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    int written = dprintf(descriptor,
        "{\"wall_seconds\":%.9f,\"user_seconds\":%.6f,\"system_seconds\":%.6f,\"maxrss_kib\":%ld,\"minor_faults\":%ld,\"major_faults\":%ld,\"exit_code\":%d}\n",
        wall, seconds(usage.ru_utime), seconds(usage.ru_stime), usage.ru_maxrss, usage.ru_minflt, usage.ru_majflt, exit_code);
    int closed = close(descriptor);
    return written < 0 || closed ? 2 : exit_code;
}
