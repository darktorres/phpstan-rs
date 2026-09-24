<?php
            /** @template T */
            trait Repo {}
            /** @use Repo<int, string, bool> */
            class UserRepo {
                use Repo;
            }
        