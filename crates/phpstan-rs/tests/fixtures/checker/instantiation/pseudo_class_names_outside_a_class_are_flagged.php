<?php
            function f(): void {
                $a = new self();
                $b = new static();
                $c = new parent();
            }
        