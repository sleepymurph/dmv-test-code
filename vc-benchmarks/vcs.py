#!/usr/bin/env python

import os
import shutil
import subprocess
import sys
import tempfile
import trialutil
import unittest

from trialutil import log,logcall

class AbstractRepo(object):

    def __init__(self, workdir):
        self.workdir = workdir

    def run_cmd(self, cmd):
        logcall(cmd, cwd=self.workdir, shell=True)

    def check_output(self, cmd):
        return subprocess.check_output( cmd, cwd=self.workdir, shell=True)

    def check_total_size(self):
        du_out = self.check_output("du -s --block-size=1 .")
        bytecount = du_out.strip().split()[0]
        return int(bytecount)


class SimpleCopyRepo(AbstractRepo):

    @staticmethod
    def check_version():
        return "Dummy copy repo -- version 1"

    def __init__(self, workdir):
        super(SimpleCopyRepo, self).__init__(workdir)

    def init_repo(self):
        self.run_cmd("mkdir .backup")

    def start_tracking_file(self, filename):
        pass

    def commit_file(self, filename):
        commit_id = self.get_last_commit_id()
        self.run_cmd("cp -r %s .backup/" % filename)
        if not commit_id:
            commit_id = 1
        else:
            commit_id = int(commit_id)+1
        with open(os.path.join(self.workdir,".backup/_commit_id"), "w") as f:
            f.write(str(commit_id))

    def check_status(self, filename):
        log("Status check not implemented for dummy repo.")

    def garbage_collect(self):
        log("Dummy repo has no garbage collection.")

    def get_last_commit_id(self):
        try:
            with open(os.path.join(self.workdir,".backup/_commit_id")) as f:
                commit_id = f.read()
            return commit_id
        except IOError:
            return None

    def is_file_in_commit(self, commit_id, filename):
        return os.path.exists(os.path.join(self.workdir, filename))

    def check_repo_integrity(self):
        return not os.path.exists(os.path.join(self.workdir, ".backup/_assume_repo_corrupt"))

    def corrupt_repo(self):
        self.run_cmd("touch .backup/_assume_repo_corrupt")


class GitRepo(AbstractRepo):

    @staticmethod
    def check_version():
        return subprocess.check_output("git --version", shell=True).strip()

    def __init__(self, workdir):
        super(GitRepo, self).__init__(workdir)

    def init_repo(self):
        self.run_cmd("git init")

    def start_tracking_file(self, filename):
        pass

    def commit_file(self, filename):
        self.run_cmd("git add %s" % filename)
        self.run_cmd("git commit -m 'Add %s'" % filename)
        log("Commit finished")

    def check_status(self, filename):
        self.run_cmd("git status %s" % filename)

    def garbage_collect(self):
        self.run_cmd("git gc")
        log("GC finished")

    def get_last_commit_id(self):
        try:
            return self.check_output("git rev-parse HEAD").strip()
        except subprocess.CalledProcessError:
            return None

    def is_file_in_commit(self, commit_id, filename):
        try:
            # NOTE: This will only find files in the top level of the tree
            # TODO: Switch to git ls-files?
            output = self.check_output(
                                "git ls-tree -r %s | grep '\t%s$'"
                                % (commit_id, filename)).strip()
            return bool(output)
        except subprocess.CalledProcessError:
            return False

    def check_repo_integrity(self):
        try:
            self.run_cmd("git fsck")
            return True
        except trialutil.CallFailedError:
            return False

    def corrupt_repo(self):
        internal_file = self.check_output(
                            "find .git/objects -type f | head -n1").strip()
        self.run_cmd("chmod u+w %s" % internal_file)
        trialutil.make_small_edit(self.workdir, internal_file, 10)


class HgRepo(AbstractRepo):

    @staticmethod
    def check_version():
        return subprocess.check_output("hg version | head -n 1", shell=True
                ).strip()

    def __init__(self, workdir):
        super(HgRepo, self).__init__(workdir)

    def init_repo(self):
        self.run_cmd("hg init")

    def start_tracking_file(self, filename):
        self.run_cmd("hg add %s" % filename)
        log("Tracking test file %s" % filename)

    def commit_file(self, filename):
        self.run_cmd("hg commit -m 'Add %s'" % filename)
        log("Commit finished")

    def check_status(self, filename):
        self.run_cmd("hg status %s" % filename)

    def garbage_collect(self):
        log("HG has no garbage collection")

    def get_last_commit_id(self):
        revid = self.check_output("hg id -i").strip()
        if revid in ["000000000000", "000000000000+"]:
            return None
        else:
            return revid

    def is_file_in_commit(self, commit_id, filename):
        try:
            output = self.check_output(
                                "hg manifest -r %s | grep '^%s$'"
                                % (commit_id, filename)).strip()
            return bool(output)
        except subprocess.CalledProcessError:
            return False

    def check_repo_integrity(self):
        try:
            self.run_cmd("hg verify")
            return True
        except trialutil.CallFailedError:
            return False

    def corrupt_repo(self):
        internal_file = self.check_output(
                            "find .hg/store/data -type f | head -n1").strip()
        trialutil.make_small_edit(self.workdir, internal_file, 10)


class BupRepo(AbstractRepo):

    @staticmethod
    def check_version():
        return subprocess.check_output("bup --version", shell=True).strip()

    def __init__(self, workdir):
        super(BupRepo, self).__init__(workdir)
        self.workdir = workdir
        self.repodir = os.path.join(workdir, ".bup")
        self.env = os.environ.copy()
        self.env['BUP_DIR'] = self.repodir
        self.env['GIT_DIR'] = self.repodir
        self.branchname = "test_run"

    def run_cmd(self, cmd):
        logcall(cmd, cwd=self.workdir, shell=True, env=self.env)

    def check_output(self, cmd):
        return subprocess.check_output( cmd, cwd=self.workdir,
                shell=True, env=self.env)

    def init_repo(self):
        self.run_cmd("bup init")

    def start_tracking_file(self, filename):
        pass

    def commit_file(self, filename):
        self.run_cmd("bup index %s" % filename)
        self.run_cmd("bup save -n '%s' %s" % (self.branchname,filename))
        log("Commit finished")

    def check_status(self, filename):
        self.run_cmd("bup index %s" % filename)
        self.run_cmd("bup index --status %s" % filename)

    def garbage_collect(self):
        log("Bup has no garbage collection")

    def get_last_commit_id(self):
        try:
            return self.check_output("git rev-parse %s" % self.branchname).strip()
        except subprocess.CalledProcessError:
            return None

    def is_file_in_commit(self, commit_id, filename):
        try:
            # We're using `git ls-tree` here because `bup ls` does not take
            # commit ids, only paths like <backup_name>/<date>/<rest_of_path>.
            # However, by using a Git util we're going to see Git's view of the
            # data, where Bup has split files into chunks named
            # <file_name>.bup/<hex_chunk_ids>. So we have to account for that
            # in the grep regex.
            output = self.check_output(
                                "git ls-tree -r %s | grep -E '/%s(\.bup/[0-9a-f/]+)?$'"
                                % (commit_id, filename)).strip()
            return bool(output)
        except subprocess.CalledProcessError:
            return False

    def check_repo_integrity(self):
        try:
            self.run_cmd("git fsck")
            return True
        except trialutil.CallFailedError:
            return False

    def corrupt_repo(self):
        internal_file = self.check_output(
                            "find .bup/objects -name '*.pack' | head -n1"
                            ).strip()
        trialutil.make_small_edit(self.workdir, internal_file, 20)


class PrototypeRepo(AbstractRepo):

    @staticmethod
    def check_version():
        return subprocess.check_output(
                "prototype --version", shell=True).strip()

    def __init__(self, workdir):
        super(PrototypeRepo, self).__init__(workdir)

    def init_repo(self):
        self.run_cmd("prototype init")

    def start_tracking_file(self, filename):
        # Not necessary. Files are tracked by default.
        pass

    def commit_file(self, filename):
        self.run_cmd("prototype commit -m 'Add %s'" % filename)
        log("Commit finished")

    def check_status(self, filename):
        self.run_cmd("prototype status")

    def garbage_collect(self):
        log("Prototype has no garbage collection")

    def get_last_commit_id(self):
        try:
            revid = self.check_output("prototype parents | head -n1").strip()
            if revid in [""]:
                return None
            else:
                return revid
        except subprocess.CalledProcessError:
            return None

    def is_file_in_commit(self, commit_id, filename):
        try:
            output = self.check_output(
                                "prototype ls-files %s | grep '^%s$'"
                                % (commit_id, filename)).strip()
            return bool(output)
        except subprocess.CalledProcessError:
            return False

    def check_repo_integrity(self):
        try:
            self.run_cmd("prototype fsck")
            return True
        except trialutil.CallFailedError:
            return False

    def corrupt_repo(self):
        internal_file = self.check_output(
                            "find .prototype/objects -type f | head -n1"
                            ).strip()
        trialutil.make_small_edit(self.workdir, internal_file, 10)


vcschoices = {
            'copy': SimpleCopyRepo,
            'git': GitRepo,
            'hg': HgRepo,
            'bup': BupRepo,
            'prototype': PrototypeRepo
        }


class AbstractRepoTests(object):
    """ A set of tests for the repository interfaces

        Subclasses to test actual repo classes should define self.repo_class in
        their __init__ methods.
    """

    def setUp(self):
        self.tempdir = tempfile.mkdtemp(prefix='vcs_py_unittest_')

    def tearDown(self):
        shutil.rmtree(self.tempdir)

    def test_check_total_size_empty(self):
        repo = self.repo_class(self.tempdir)
        size = repo.check_total_size()
        one_filesystem_block_size = 4096
        self.assertEqual(size, one_filesystem_block_size)

    def test_init_empty(self):
        repo = self.repo_class(self.tempdir)
        repo.init_repo()
        self.assertEqual(repo.get_last_commit_id(), None)

        size = repo.check_total_size()
        one_filesystem_block_size = 4096
        self.assertNotEqual(size, one_filesystem_block_size)

    def test_commit(self):
        repo = self.repo_class(self.tempdir)
        repo.init_repo()
        trialutil.create_file(self.tempdir, "test_file", 10)
        repo.start_tracking_file("test_file")
        repo.commit_file("test_file")

        commitid = repo.get_last_commit_id()

        self.assertNotEqual(commitid, None)
        self.assertTrue(repo.is_file_in_commit(commitid, "test_file"))
        self.assertFalse(repo.is_file_in_commit(commitid, "test_fil"))
        self.assertFalse(repo.is_file_in_commit(commitid, "est_file"))

    def test_commit_large_file(self):
        repo = self.repo_class(self.tempdir)
        repo.init_repo()
        trialutil.create_file(self.tempdir, "test_file", 128 * 2**10)
        repo.start_tracking_file("test_file")
        repo.commit_file("test_file")

        commitid = repo.get_last_commit_id()

        self.assertNotEqual(commitid, None)
        self.assertTrue(repo.is_file_in_commit(commitid, "test_file"))
        self.assertFalse(repo.is_file_in_commit(commitid, "test_fil"))
        self.assertFalse(repo.is_file_in_commit(commitid, "est_file"))

    def test_integrity_check(self):
        repo = self.repo_class(self.tempdir)
        repo.init_repo()
        trialutil.create_file(self.tempdir, "test_file", 10)
        repo.start_tracking_file("test_file")
        repo.commit_file("test_file")

        self.assertTrue(repo.check_repo_integrity())

        repo.corrupt_repo()
        self.assertFalse(repo.check_repo_integrity())


class SimpleCopyRepoTests(AbstractRepoTests, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        super(SimpleCopyRepoTests,self).__init__(*args, **kwargs)
        self.repo_class = SimpleCopyRepo

class GitTests(AbstractRepoTests, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        super(GitTests,self).__init__(*args, **kwargs)
        self.repo_class = GitRepo

class HgTests(AbstractRepoTests, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        super(HgTests,self).__init__(*args, **kwargs)
        self.repo_class = HgRepo

class BupTests(AbstractRepoTests, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        super(BupTests,self).__init__(*args, **kwargs)
        self.repo_class = BupRepo

class PrototypeTests(AbstractRepoTests, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        super(PrototypeTests,self).__init__(*args, **kwargs)
        self.repo_class = PrototypeRepo

if __name__ == '__main__':
    unittest.main()
