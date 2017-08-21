DMV: Distributed Media Versioning -- Test Code
==================================================

DMV is a project to generalize version control beyond source code and into
larger files such as photos and video, and also into larger collections that
might not fit on one disk. It hopes to be a cross between a version control
system and a generalized distributed data store.


Test Code
==================================================

This repository contains test code and helper scripts for the DMV project,
including simple scripts to examine Git repositories, and benchmark experiments
that test the limits of version control systems.

It is a mix of Python, Perl, and Bash scripts. Nothing too fancy. They should
run on vanilla Linux systems (I developed them on Debian 8, Jessie).


Scripts
--------------------------------------------------

- `helper-scripts` -- Used for research and for creating diagrams for the paper

    - `git-dot` -- A Perl script that examines a Git repository and spits out a
      Graphviz diagram of its DAG

    - `git-examine-object` -- A wrapper around `git cat-file` to print more
      information all at once

- `vc-benchmarks` -- Experiment scripts, see the "VCS Scaling Experiments" of
  the DMV master's thesis for explanation and results

    - `increasing_file_size.py` -- The first of the main VCS Scaling
      Experiments. Commits a single increasingly large file to the target
      repository and records metrics like commit time and CPU usage.

    - `increasing_number_of_files.py` -- The second of the main VCS Scaling
      Experiments. Commits an increasing number of files to the target
      repository and records metrics like commit time and CPU usage.

    - `filesystem_limit_micro.py` -- An experiment script to test an object
      directory layouts and how much they waste inodes. Writes random objects
      according to that layout until the disk is full, collecting write time
      metrics along the way.

    - `filesystem_limit_multi.sh` -- A shell script to run the
      `filesystem_limit_micro.py` script repeatedly with different object
      directory layouts.

    - `trialenv.py` -- A Python module that helps the benchmark scripts capture
      information about the environment they're running in, which is then
      printed as a header in the data file.

    - `trialutil.py` -- A Python module with common routines shared by the
      different experiment scripts.

    - `trialenv.py` -- A Python module that abstracts different version control
      systems behind a common interface so that they can be easily plugged into
      the experiment scripts.

    - `sudoers-user-reformat` -- Deleting a large number of files from an ext4
      filesystem is a very slow operation. It's much faster for the experiment
      scripts to reformat and remount the test partition than to recursively
      delete the temporary directory. In order to do that reformatting, the
      experiment scripts need to be able to run the reformat commands via `sudo`
      without a password. This file is a a chunk of `sudo` configuration that
      can be copied to your `/etc/sudoers.d/` directory to give the script that
      no-password reformatting access.

Note that for much of development, this repository and the DMV Source Code
repository were combined. The `helper-scripts` and `vc-benchmark` subdirectories
were the same, but there was also a `prototype` directory that held the DMV
prototype. So do not be alarmed if you check out old code and find the prototype
appearing in this repository.


Running
--------------------------------------------------

Each of the experiment Python scripts use the `argparse` module to provide
command-line argument parsing. So you can see available options with
`./increasing_file_size.py --help`. The command line used with each experiment
run is also included in the output, along with other environment and program
version data, so that results can be checked and experiments can be duplicated.

The scripts also print their own command lines in their data files, so you can
look at previous data files to see how to duplicate the experiment.


Data
--------------------------------------------------

Experiment results data used in the report is in the [DMV Publications
repository]( https://github.com/sleepymurph/dmv-publications).


More About DMV
==================================================

DMV hopes to extend the distributed part of the distributed version control
concept so that the actual collection/history can be distributed across several
repositories, making it easy to transfer the files you need to the locations
where you need them and to keep everything synchronized.

DMV was created as a master's thesis project at the University of Tromsø,
Norway's Arctic University, by a student named Mike Murphy (that's me). The
prototype is definitely not ready for prime time yet, but I do think I'm on to
something here.


Documentation and other related repositories
--------------------------------------------------

At this point the best source of documentation for the project is the master's
thesis itself. An archived PDF version of the thesis is available in Munin, the
University of Tromsø's open research archive
(<http://hdl.handle.net/10037/11213>).

Beyond that there are three source repositories of interest:

1. [DMV Source Code]( https://github.com/sleepymurph/dmv), the prototype source
   code itself.
2. [DMV Publications]( https://github.com/sleepymurph/dmv-publications), LaTeX
   and other materials used to generate publication PDFs, including the master's
   thesis itself and presentation slides. Also includes experiment data.
3. [DMV Test Code]( https://github.com/sleepymurph/dmv-test-code), including
   helpers scripts used in my research and experiment/benchmark scripts.

I welcome any feedback or questions at <dmv@sleepymurph.com>.
