<?php
            function f(string $x) {
                filter_var($x, FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE);
            }
        