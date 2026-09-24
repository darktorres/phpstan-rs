<?php
            /** @template T */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int> */
                use Repo;
            }
        