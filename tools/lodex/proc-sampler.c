/* Bounded Linux /proc sampler. No child processes, credentials or transcripts.
 * Output is accepted by footprint.py. A vanished/reused PID ends the capture.
 * Usage: proc-sampler PID COUNT INTERVAL_MS
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int number(const char *text, unsigned long max, unsigned long *value) {
    char *end;
    errno = 0;
    *value = strtoul(text, &end, 10);
    return !errno && text[0] >= '0' && text[0] <= '9' && !*end &&
           *value > 0 && *value <= max;
}

static int read_stat(const char *path, char *line, size_t capacity,
                     unsigned long long *identity) {
    FILE *file = fopen(path, "r");
    if (!file) return 0;
    char *result = fgets(line, (int)capacity, file);
    fclose(file);
    if (!result || !strchr(line, '\n')) return -1;
    const char *fields = strrchr(line, ')');
    if (!fields || fields[1] != ' ' || !fields[2]) return -1;
    fields += 2;
    if (*fields == 'Z' || *fields == 'X') return 0;
    /* Field 3 is state; starttime is field 22. Do not parse the comm field. */
    for (int field = 3; field < 22; ++field) {
        fields = strchr(fields, ' ');
        if (!fields) return -1;
        while (*fields == ' ') ++fields;
    }
    char *end;
    errno = 0;
    *identity = strtoull(fields, &end, 10);
    return errno || end == fields ? -1 : 1;
}

int main(int argc, char **argv) {
    unsigned long pid, count, milliseconds;
    if (argc != 4 || !number(argv[1], INT_MAX, &pid) ||
        !number(argv[2], 36000, &count) ||
        !number(argv[3], 60000, &milliseconds) || milliseconds < 10) {
        fputs("usage: proc-sampler PID COUNT INTERVAL_MS (10..60000)\n", stderr);
        return 2;
    }
    char stat_path[64], status_path[64];
    snprintf(stat_path, sizeof(stat_path), "/proc/%lu/stat", pid);
    snprintf(status_path, sizeof(status_path), "/proc/%lu/status", pid);
    unsigned long long identity = 0;
    struct timespec deadline;
    if (clock_gettime(CLOCK_MONOTONIC, &deadline)) return 1;
    for (unsigned long sample = 0; sample < count; ++sample) {
        char stat_line[4096], line[512];
        unsigned long long observed;
        int state = read_stat(stat_path, stat_line, sizeof(stat_line), &observed);
        if (state != 1 || (sample && observed != identity))
            return sample && state >= 0 ? 0 : 1;
        identity = observed;
        FILE *status = fopen(status_path, "r");
        if (!status) return sample ? 0 : 1;
        unsigned long rss = 0, peak = 0, swap = 0, threads = 0, anon = 0, file = 0;
        unsigned int found = 0;
        while (fgets(line, sizeof(line), status)) {
            if (sscanf(line, "VmRSS: %lu", &rss) == 1) found |= 1;
            else if (sscanf(line, "VmHWM: %lu", &peak) == 1) found |= 2;
            else if (sscanf(line, "VmSwap: %lu", &swap) == 1) found |= 4;
            else if (sscanf(line, "Threads: %lu", &threads) == 1) found |= 8;
            else if (sscanf(line, "RssAnon: %lu", &anon) == 1) found |= 16;
            else if (sscanf(line, "RssFile: %lu", &file) == 1) found |= 32;
        }
        fclose(status);
        if ((found & 15) != 15) return sample ? 0 : 1;
        printf("sample %lld\nstat %sstatus VmRSS: %lu\nstatus VmHWM: %lu\n"
               "status VmSwap: %lu\nstatus Threads: %lu\n",
               (long long)time(NULL), stat_line, rss, peak, swap, threads);
        if (found & 16) printf("status RssAnon: %lu\n", anon);
        if (found & 32) printf("status RssFile: %lu\n", file);
        puts("end");
        if (fflush(stdout)) return 1;
        if (sample + 1 == count) break;
        deadline.tv_sec += milliseconds / 1000;
        deadline.tv_nsec += (long)(milliseconds % 1000) * 1000000L;
        if (deadline.tv_nsec >= 1000000000L) {
            ++deadline.tv_sec;
            deadline.tv_nsec -= 1000000000L;
        }
        int error;
        do {
            error = clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &deadline, NULL);
        } while (error == EINTR);
        if (error) return 1;
    }
    return 0;
}
