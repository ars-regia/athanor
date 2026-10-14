/*
 * Build-time check of athanor-agent-helper-cancel-before-answer.patch: drives the PAM
 * conversation of polkit-agent-helper-1, compiled from the patched source, with an agent
 * that hangs up. Run by %check of polkit.spec; exits non-zero when a case fails.
 *
 *   - a hang-up before any answer ends the helper with status 2, so the auth stack never
 *     reaches pam_faillock's authfail (polkit-org/polkit#713);
 *   - a hang-up after an answer, with or without its newline, fails the conversation with
 *     PAM_CONV_ERR as upstream does, so a tried password is still counted.
 */
#define main helper_main
#include "polkitagenthelper-pam.c"
#undef main

#include <sys/wait.h>

/* helper_main's callees in polkitagenthelperprivate.c; the cases never call helper_main. */
int _polkit_clearenv (void) { abort (); }
char *read_cookie (int argc, char **argv) { (void) argc; (void) argv; abort (); }
gboolean send_dbus_message (const char *cookie, const char *user, int pidfd, int uid)
{
  (void) cookie; (void) user; (void) pidfd; (void) uid;
  abort ();
}
void flush_and_wait (void) { abort (); }

static int
prompt (struct pam_response **resp)
{
  struct pam_message m = { PAM_PROMPT_ECHO_OFF, "Password: " };
  const struct pam_message *msg = &m;
  return conversation_function (1, &msg, resp, NULL);
}

/* Runs CASE in a child whose stdin holds AGENT_SENDS and then end of file, and whose
 * stdout (the prompts) goes to /dev/null. Returns the child's wait status. */
static int
run (const char *agent_sends, int (*case_fn) (void))
{
  int fds[2];
  pid_t pid;
  int status;

  if (pipe (fds) != 0)
    abort ();
  if (write (fds[1], agent_sends, strlen (agent_sends)) != (ssize_t) strlen (agent_sends))
    abort ();
  close (fds[1]);
  fflush (stdout);
  pid = fork ();
  if (pid < 0)
    abort ();
  if (pid == 0)
    {
      if (dup2 (fds[0], STDIN_FILENO) < 0 || freopen ("/dev/null", "w", stdout) == NULL)
        _exit (125);
      _exit (case_fn ());
    }
  close (fds[0]);
  if (waitpid (pid, &status, 0) != pid)
    abort ();
  return status;
}

/* Reaches _exit inside the conversation when the patch holds. */
static int
hang_up_before_answer (void)
{
  struct pam_response *resp = NULL;
  int rc = prompt (&resp);
  fprintf (stderr, "conversation returned %d on a hang-up before any answer\n", rc);
  return 1;
}

static int
answer_then_hang_up (const char *expected)
{
  struct pam_response *resp = NULL;
  int rc = prompt (&resp);
  if (rc != PAM_SUCCESS || resp == NULL || strcmp (resp[0].resp, expected) != 0)
    {
      fprintf (stderr, "first prompt: rc %d, no answer \"%s\"\n", rc, expected);
      return 1;
    }
  resp = NULL;
  rc = prompt (&resp);
  if (rc != PAM_CONV_ERR || resp != NULL)
    {
      fprintf (stderr, "hang-up after an answer: rc %d, not PAM_CONV_ERR\n", rc);
      return 1;
    }
  return 0;
}

static int line_then_hang_up (void) { return answer_then_hang_up ("guess"); }
static int partial_then_hang_up (void) { return answer_then_hang_up ("gu"); }
static int empty_then_hang_up (void) { return answer_then_hang_up (""); }

static int
expect (const char *name, int status, int code)
{
  if (WIFEXITED (status) && WEXITSTATUS (status) == code)
    {
      printf ("ok: %s\n", name);
      return 0;
    }
  printf ("FAIL: %s: wait status %d, expected exit %d\n", name, status, code);
  return 1;
}

int
main (void)
{
  int fail = 0;
  fail |= expect ("hang-up before any answer ends the helper with status 2",
                  run ("", hang_up_before_answer), 2);
  fail |= expect ("hang-up after an answer fails the conversation",
                  run ("guess\n", line_then_hang_up), 0);
  fail |= expect ("hang-up after an answer without newline fails the conversation",
                  run ("gu", partial_then_hang_up), 0);
  fail |= expect ("hang-up after an empty answer fails the conversation",
                  run ("\n", empty_then_hang_up), 0);
  return fail;
}
