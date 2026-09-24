<?php
            interface Shape {}
            class Square implements Shape {}
            class NotAShape {}
            /** @template T of Shape */
            trait Repo {}
            class UserRepo {
                /** @use Repo<NotAShape> */
                use Repo;
            }
        