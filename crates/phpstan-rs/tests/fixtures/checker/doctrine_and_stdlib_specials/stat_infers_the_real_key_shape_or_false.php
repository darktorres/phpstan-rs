<?php
            /**
             * @return array{dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false
             */
            function f(string $path) {
                return stat($path);
            }
        