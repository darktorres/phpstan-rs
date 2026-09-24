<?php
            /** @template K
             *   @template V
             */
            trait Repo {}
            class UserRepo {
                /** @use Repo<int> */
                use Repo;
            }
        