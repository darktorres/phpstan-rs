<?php
            function make(int $unused) {
                return function () use ($unused) {
                    return function ($unused) {
                        return $unused;
                    };
                };
            }
        