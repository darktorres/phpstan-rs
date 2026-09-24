<?php
            function fillErrors(array &$errors): void {}
            function test(): void {
                $errors = [];
                fillErrors($errors);
                echo $errors[0];
            }
        