<?php
            function make(): callable {
                return function () use ($this) {
                    return $this;
                };
            }
        