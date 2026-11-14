// SPDX-License-Identifier: GPL-2.0-only
/* Serialize access to one SDK process. Its own $? supplies command status. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pty.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define SOCKET_PATH "/var/run/bcm-sdk.sock"
#define READY_PATH "/var/run/bcm-sdk.ready"
#define MAX_OUTPUT (512 * 1024)
#define MAX_COMMAND 2048

static volatile sig_atomic_t stopping;
static void stop(int signal_number) { (void)signal_number; stopping = 1; }

static time_t monotonic_seconds(void)
{
	struct timespec now;
	clock_gettime(CLOCK_MONOTONIC, &now);
	return now.tv_sec;
}

static int transfer(int fd, void *buffer, size_t length, bool sending)
{
	char *cursor = buffer;
	while (length) {
		ssize_t n = sending ? write(fd, cursor, length) : read(fd, cursor, length);
		if (n < 0 && errno == EINTR) continue;
		if (n <= 0) return -1;
		cursor += n; length -= n;
	}
	return 0;
}

static void reply(int fd, int status, const char *output, size_t length)
{
	uint32_t header[] = { htonl((uint32_t)status), htonl((uint32_t)length) };
	if (!transfer(fd, header, sizeof(header), true))
		(void)transfer(fd, (void *)output, length, true);
	close(fd);
}

static void quote_json(const char *text)
{
	putchar('"');
	for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
		if (*p == '"' || *p == '\\') printf("\\%c", *p);
		else if (*p < 32) printf("\\u%04x", *p);
		else putchar(*p);
	}
	putchar('"');
}

static int client(int argc, char **argv)
{
	bool json = argc > 1 && !strcmp(argv[1], "--json");
	int first = json ? 2 : 1, status = 1, fd = -1;
	char command[MAX_COMMAND] = "", *output = NULL;
	const char *error = "SDK service unavailable";
	struct sockaddr_un address = { .sun_family = AF_UNIX, .sun_path = SOCKET_PATH };
	struct timeval timeout = { .tv_sec = 125 };
	uint32_t header[2];
	if (first >= argc) { error = "usage: bcmctl [--json] SDK command"; goto done; }
	for (int i = first; i < argc; i++) {
		if (strchr(argv[i], '\n') || strchr(argv[i], '\r') ||
		    strlen(command) + strlen(argv[i]) + 2 >= sizeof(command)) {
			error = "command is invalid or too long"; goto done;
		}
		if (i != first) strcat(command, " ");
		strcat(command, argv[i]);
	}
	strcat(command, "\n");
	fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof(address))) goto done;
	setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
	setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
	if (transfer(fd, command, strlen(command), true) ||
	    transfer(fd, header, sizeof(header), false)) {
		error = "SDK request failed or timed out"; goto done;
	}
	if (ntohl(header[1]) > MAX_OUTPUT) { error = "oversized SDK reply"; goto done; }
	output = calloc(1, ntohl(header[1]) + 1);
	if (!output) { error = "out of memory"; goto done; }
	if (transfer(fd, output, ntohl(header[1]), false)) {
		free(output); output = NULL; error = "incomplete SDK reply"; goto done;
	}
	status = (int32_t)ntohl(header[0]);
done:
	if (fd >= 0) close(fd);
	if (json) {
		printf("{\"ok\":%s,\"status\":%d,\"output\":", status ? "false" : "true", status);
		quote_json(output ? output : error);
		puts("}");
	} else {
		fputs(output ? output : error, status ? stderr : stdout);
		if (!output) fputc('\n', stderr);
	}
	free(output);
	return status ? 1 : 0;
}

static bool at_prompt(const char *output, size_t length)
{
	while (length && (output[length - 1] == ' ' || output[length - 1] == '\r' || output[length - 1] == '\n')) length--;
	return length >= 6 && !memcmp(output + length - 6, "BCM.0>", 6);
}

static size_t omit_marker_lines(char *text, size_t length, const char *marker)
{
	size_t read_position = 0, write_position = 0;
	while (read_position < length) {
		char *line = text + read_position;
		char *end = memchr(line, '\n', length - read_position);
		size_t bytes = end ? (size_t)(end - line) + 1 : length - read_position;
		if (!memmem(line, bytes, marker, strlen(marker))) {
			memmove(text + write_position, line, bytes);
			write_position += bytes;
		}
		read_position += bytes;
	}
	return write_position;
}

static int server(const char *sdk, const char *directory)
{
	struct sockaddr_un address = { .sun_family = AF_UNIX, .sun_path = SOCKET_PATH };
	struct winsize dimensions = { .ws_row = 40, .ws_col = 200 };
	int master, listener, logfd, requester = -1;
	pid_t child = forkpty(&master, NULL, NULL, &dimensions);
	char *output = calloc(1, MAX_OUTPUT + 1), marker[80] = "";
	size_t length = 0;
	bool ready = false, announced = false, child_alive = true;
	unsigned int sequence = 0;
	time_t deadline = 0;
	if (child < 0 || !output) {
		if (child > 0) { kill(child, SIGKILL); waitpid(child, NULL, 0); }
		return 1;
	}
	if (!child) {
		if (chdir(directory)) _exit(126);
		setenv("TERM", "dumb", 1);
		execl(sdk, sdk, NULL);
		_exit(127);
	}
	signal(SIGTERM, stop); signal(SIGINT, stop); signal(SIGPIPE, SIG_IGN);
	fcntl(master, F_SETFD, FD_CLOEXEC);
	listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	logfd = open("/tmp/bcm-sdk/console.txt", O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
	unlink(SOCKET_PATH); unlink(READY_PATH);
	umask(0077);
	if (listener < 0 || bind(listener, (struct sockaddr *)&address, sizeof(address)) || listen(listener, 8)) {
		perror("SDK socket"); stopping = 1;
	}
	while (!stopping) {
		struct pollfd descriptors[] = {
			{ .fd = master, .events = POLLIN },
			{ .fd = listener, .events = ready && requester < 0 ? POLLIN : 0 }
		};
		pid_t exited;
		while ((exited = waitpid(-1, NULL, WNOHANG)) > 0)
			if (exited == child) { stopping = 1; child_alive = false; }
		if (stopping) break;
		int events = poll(descriptors, 2, 200);
		if (events < 0 && errno != EINTR) break;
		if (descriptors[0].revents & POLLIN) {
			char chunk[4096];
			ssize_t n = read(master, chunk, sizeof(chunk));
			if (n <= 0) break;
			if (logfd >= 0) {
				struct stat information;
				(void)transfer(logfd, chunk, n, true);
				if (!fstat(logfd, &information) && information.st_size >= 4 * 1024 * 1024) {
					close(logfd);
					rename("/tmp/bcm-sdk/console.txt", "/tmp/bcm-sdk/console.prev.txt");
					logfd = open("/tmp/bcm-sdk/console.txt", O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
				}
			}
			if (length + n > MAX_OUTPUT) {
				if (requester >= 0) { reply(requester, 1, "SDK output limit exceeded\n", 26); requester = -1; }
				length = 0;
			}
			memcpy(output + length, chunk, n); length += n; output[length] = 0;
			if (!announced && at_prompt(output, length)) {
				bool initialized = strstr(output, "rc: BCM driver initialized") &&
					strstr(output, "rc: Port modes initialized") &&
					!strstr(output, "ERROR loading rc script");
                char generation[80];
                int generation_length = snprintf(generation, sizeof(generation), "%ld-%lld\n", (long)getpid(), (long long)monotonic_seconds());
                int generation_fd = open("/tmp/bcm-sdk/native-generation", O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
                if (generation_fd >= 0) { (void)transfer(generation_fd, generation, generation_length, true); close(generation_fd); }
				int fd = open(READY_PATH, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
				if (fd >= 0) { (void)transfer(fd, initialized ? "ready\n" : "failed\n", initialized ? 6 : 7, true); close(fd); }
				announced = ready = true; length = 0; output[0] = 0;
				/* The apply hook uses this same socket, so it must run asynchronously. */
				if (initialized && !access("/usr/libexec/bcm-switch", X_OK) && !fork()) {
					execl("/usr/libexec/bcm-switch", "bcm-switch", "apply", NULL);
					_exit(127);
				}
			}
			if (requester >= 0) {
				char *found = strstr(output, marker);
				while (found && found != output && found[-1] != '\n') found = strstr(found + 1, marker);
				if (found) {
					char *end = strchr(found, '\n');
					if (end) {
						int status = (int)strtol(found + strlen(marker), NULL, 10);
						size_t bytes = omit_marker_lines(output, found - output, marker);
						reply(requester, status, output, bytes);
						requester = -1; ready = true; length = 0; output[0] = 0;
					}
				}
			}
		}
		if (descriptors[0].revents & (POLLHUP | POLLERR)) break;
		if (requester >= 0 && monotonic_seconds() >= deadline) {
			reply(requester, 1, "SDK command timed out\n", 22); requester = -1;
			/* Preserve ownership after a timeout; resume only at a real SDK prompt. */
			ready = at_prompt(output, length);
		}
		if (!ready && announced && requester < 0 && at_prompt(output, length)) ready = true;
		if (descriptors[1].revents & POLLIN) {
			char command[MAX_COMMAND], batch[MAX_COMMAND + 128];
			struct timeval timeout = { .tv_sec = 5 };
			size_t used = 0;
			int fd = accept4(listener, NULL, NULL, SOCK_CLOEXEC);
			if (fd < 0) continue;
			setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
			setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
			while (used < sizeof(command) - 1) {
				ssize_t n = read(fd, command + used, 1);
				if (n != 1 || command[used] == '\n') break;
				used++;
			}
			command[used] = 0;
			if (!used || used == sizeof(command) - 1 || strchr(command, '\r')) {
				reply(fd, 1, "invalid command\n", 16); continue;
			}
			snprintf(marker, sizeof(marker), "__BCM_DONE_%ld_%u__", (long)getpid(), ++sequence);
			snprintf(batch, sizeof(batch), "%s\necho %s $?\n", command, marker);
			length = 0; output[0] = 0; ready = false;
			requester = fd; deadline = monotonic_seconds() + 120;
			if (transfer(master, batch, strlen(batch), true)) break;
		}
	}
	if (requester >= 0) reply(requester, 1, "SDK service stopped\n", 20);
	unlink(READY_PATH); unlink(SOCKET_PATH);
	if (listener >= 0) close(listener);
	if (logfd >= 0) close(logfd);
	close(master);
	if (child_alive) {
		kill(child, SIGTERM);
		for (int i = 0; i < 20 && waitpid(child, NULL, WNOHANG) == 0; i++) usleep(100000);
		if (waitpid(child, NULL, WNOHANG) == 0) { kill(child, SIGKILL); waitpid(child, NULL, 0); }
	}
	free(output);
	return 1;
}

int main(int argc, char **argv)
{
	const char *name = strrchr(argv[0], '/'); name = name ? name + 1 : argv[0];
	if (!strcmp(name, "bcmd")) {
		if (argc != 3) { fputs("usage: bcmd SDK-executable working-directory\n", stderr); return 2; }
		return server(argv[1], argv[2]);
	}
	return client(argc, argv);
}
