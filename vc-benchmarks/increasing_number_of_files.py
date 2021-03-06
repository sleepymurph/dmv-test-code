#!/usr/bin/env python

import argparse
import collections
import math
import os.path
import shutil
import tempfile

import trialenv
import trialutil
import vcs

from trialutil import *

def parse_args():
    parser = argparse.ArgumentParser(description=
            "Measure VCS performance when adding a large number of files")

    parser.add_argument("vcs", choices=vcs.vcschoices.keys(),
            help="vcs to test")

    parser.add_argument("start_mag", type=int,
            help="starting magnitude (10^N)")
    parser.add_argument("end_mag", type=int, default=-1, nargs="?",
            help="ending magnitude (10^N)")

    parser.add_argument("--mag-steps", type=int,
            default=1,
            help="steps per order of magnitude, from 10^N to 10^(N+1)")

    parser.add_argument("--each-file-mag", type=int,
            default=10,
            help="magnitude size of each file (2^N)")

    parser.add_argument("--data-gen",
            choices=['sparse', 'random'], default='sparse',
            help="data generating strategy")

    parser.add_argument("--tmp-dir", default="/tmp",
            help="directory in which to create and destroy test repos")

    parser.add_argument("--timeout", default=None,
            help="give up on subprocesses after a number of seconds")

    parser.add_argument("--reformat-partition", default=None,
            help="reformat this device instead of deleting files one-by-one")

    args = parser.parse_args()
    if args.end_mag==-1:
        args.end_mag = args.start_mag+1

    return args


class TrialStats:

    cmdmax = CmdResults.max_width()
    vermax = VerificationResults.max_width()
    cpupat = '%10d'
    timepat = '%9.3f'

    columns = [
            Column("mag", "%3d", sample=0),
            Column("filecount", "%12d", sample=0),
            Column("totalbytes", "0x%010x", sample=0),
            Column("cre_time", timepat, sample=0),

            Column("c1_time", timepat, sample=0),
            Column("c1_size", "0x%010x", sample=0),
            Column("c1_cmd", "%s", max_w=cmdmax),
            Column("c1_ver", "%s", max_w=vermax),
            Column("c1_repo", "%s", max_w=vermax),

            Column("c1_user", cpupat, sample=0),
            Column("c1_nice", cpupat, sample=0),
            Column("c1_system", cpupat, sample=0),
            Column("c1_idle", cpupat, sample=0),
            Column("c1_iowait", cpupat, sample=0),

            Column("stat1_time", timepat, sample=0),
            Column("stat1_cmd", "%s", max_w=cmdmax),
            Column("stat2_time", timepat, sample=0),
            Column("stat2_cmd", "%s", max_w=cmdmax),

            Column("c2_time", timepat, sample=0),
            Column("c2_size", "0x%010x", sample=0),
            Column("c2_cmd", "%s", max_w=cmdmax),
            Column("c2_ver", "%s", max_w=vermax),
            Column("c2_repo", "%s", max_w=vermax),

            Column("c2_user", cpupat, sample=0),
            Column("c2_nice", cpupat, sample=0),
            Column("c2_system", cpupat, sample=0),
            Column("c2_idle", cpupat, sample=0),
            Column("c2_iowait", cpupat, sample=0),

            Column("cleanup_time", timepat, sample=0),
        ]

    def __init__(self, filecount, eachbytes, **args):
        self.filecount = filecount
        self.eachbytes = eachbytes

        self.mag = math.log10(self.filecount)
        self.totalbytes = self.filecount * self.eachbytes

        self.cre_time = None

        self.c1_time = None
        self.c1_size = None
        self.c1_cmd = CmdResults.value('no_exec')
        self.c1_ver = VerificationResults.value('no_ver')
        self.c1_repo = VerificationResults.value('no_ver')

        self.c1_user = None
        self.c1_nice = None
        self.c1_system = None
        self.c1_idle = None
        self.c1_iowait = None

        self.stat1_time = None
        self.stat1_cmd = CmdResults.value('no_exec')
        self.stat2_time = None
        self.stat2_cmd = CmdResults.value('no_exec')

        self.c2_time = None
        self.c2_size = None
        self.c2_cmd = CmdResults.value('no_exec')
        self.c2_ver = VerificationResults.value('no_ver')
        self.c2_repo = VerificationResults.value('no_ver')

        self.c2_user = None
        self.c2_nice = None
        self.c2_system = None
        self.c2_idle = None
        self.c2_iowait = None

        self.cleanup_time = None


def run_trial(ts, vcsclass, data_gen, tmpdir="/tmp", reformat_partition=None):

    try:
        repodir = tempfile.mkdtemp(prefix='vcs_benchmark', dir=tmpdir)
        repo = vcsclass(repodir)
        repo.init_repo()

        with \
                StopWatch(ts, "cre_time"):
            create_many_files(
                    repodir, ts.filecount, ts.eachbytes,
                    prefix="many_files_dir", data_gen=data_gen)

        with \
                RepoVerifier(repo, ts, 'c1_repo'), \
                CommitVerifier(repo, obj=ts, attr='c1_ver'), \
                CmdResult(ts, 'c1_cmd'), \
                CpuUsageMeasurer(ts, user='c1_user', nice='c1_nice',
                        system='c1_system', idle='c1_idle', iowait='c1_iowait'), \
                StopWatch(ts, "c1_time"):
            repo.start_tracking_file("many_files_dir")
            repo.commit_file("many_files_dir")
        ts.c1_size = repo.check_total_size()

        with \
                CmdResult(ts, 'stat1_cmd'), \
                StopWatch(ts, 'stat1_time'):
            repo.check_status("many_files_dir")

        update_many_files(repodir, "many_files_dir", every_nth_file=16)

        with \
                CmdResult(ts, 'stat2_cmd'), \
                StopWatch(ts, 'stat2_time'):
            repo.check_status("many_files_dir")

        with \
                RepoVerifier(repo, ts, 'c2_repo'), \
                CommitVerifier(repo, obj=ts, attr='c2_ver'), \
                CmdResult(ts, 'c2_cmd'), \
                CpuUsageMeasurer(ts, user='c2_user', nice='c2_nice',
                        system='c2_system', idle='c2_idle', iowait='c2_iowait'), \
                StopWatch(ts, 'c2_time'):
            repo.commit_file("many_files_dir")
        ts.c2_size = repo.check_total_size()

    finally:
        log("Cleaning up trial files...")
        with StopWatch(ts, 'cleanup_time'):
            if reformat_partition:
                reformat_device(reformat_partition)
            else:
                shutil.rmtree(repodir)
        log("Removed trial files in %5.3f seconds" % ts.cleanup_time)


if __name__ == "__main__":

    args = parse_args()
    eachfilebytes = 2 ** args.each_file_mag

    tmpdir = os.path.expanduser(args.tmp_dir)
    env = trialenv.gather_environment_stats(
                dirs = [tmpdir],
            )
    vcsclass = vcs.vcschoices[args.vcs]
    vcs_version = vcsclass.check_version()

    if args.timeout:
        trialutil.global_logcall_timeout = float(args.timeout)

    comment("Committing increasingly large numbers of files")
    comment()
    comment(align_kvs({
            "data_gen": args.data_gen,
            "each_file_size": "0x%x bytes (%s)" \
                    % (eachfilebytes, hsize(eachfilebytes)),
            "vcs": args.vcs,
            "vcs_version": vcs_version,
            "reformat_partition": args.reformat_partition,
            "timeout": trialutil.global_logcall_timeout,
        }))
    comment()
    comment(align_kvs(env))
    comment()
    comment("Command results:")
    comment(align_kvs(CmdResults.descs))
    comment()
    comment("Verification results:")
    comment(align_kvs(VerificationResults.descs))
    comment()
    printheader(TrialStats.columns)

    # If reformatting, do one at the beginning to ensure all runs start
    # with the same conditions (last run might have been cancelled without
    # reformatting)
    if args.reformat_partition:
        reformat_device(args.reformat_partition)

    try:
        for filecount in base10trials(args.start_mag, args.end_mag, args.mag_steps):

            result = TrialStats(filecount, eachfilebytes)
            try:
                run_trial(
                        result,
                        vcsclass,
                        data_gen=args.data_gen,
                        tmpdir=tmpdir,
                        reformat_partition=args.reformat_partition)
            except KeyboardInterrupt:
                comment("Cancelled")
                raise
            except Exception as e:
                comment(repr(e))
            finally:
                printrow(TrialStats.columns, result)
    except KeyboardInterrupt:
        comment("Cancelled")
