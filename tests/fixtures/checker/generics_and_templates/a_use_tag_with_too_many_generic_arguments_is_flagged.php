<?php
            /** @template T */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int, string> */
                use Repo;
            }
        