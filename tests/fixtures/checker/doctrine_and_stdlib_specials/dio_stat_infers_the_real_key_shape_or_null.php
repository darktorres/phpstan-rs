<?php
            /**
             * @return array{device: int, inode: int, mode: int, nlink: int, uid: int, gid: int, device_type: int, size: int, blocksize: int, blocks: int, atime: int, mtime: int, ctime: int}|null
             */
            function f($fd) {
                return dio_stat($fd);
            }
        