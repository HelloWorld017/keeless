import { Button } from '@/components/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/dialog';
import { useExtraConfig } from '@/fragments/_providers/AppIntegrationProvider';
import { useState } from 'react';
import { DatabaseConfigFragment } from './database';
import { GeneralConfigFragment } from './general';

const builtInConfig = [
  { category: 'General', component: GeneralConfigFragment },
  { category: 'Database', component: DatabaseConfigFragment },
];

export const ConfigDialog = ({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) => {
  const extraConfig = useExtraConfig();
  const configs = [...builtInConfig, ...extraConfig];
  const [selectedCategory, setSelectedCategory] = useState(builtInConfig[0].category);
  const selectedConfig = configs.find(item => item.category === selectedCategory);
  const Content = selectedConfig?.component;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-108 gap-0 overflow-hidden p-0 sm:max-w-3xl h-full">
        <DialogHeader className="sr-only">
          <DialogTitle>Configuration</DialogTitle>
          <DialogDescription>Configure this application.</DialogDescription>
        </DialogHeader>
        <div className="grid min-h-80 grid-cols-[8rem_minmax(0,1fr)] sm:grid-cols-[10rem_minmax(0,1fr)]">
          <nav className="border-r p-2" aria-label="Configuration categories">
            {configs.map(item => (
              <Button
                key={item.category}
                type="button"
                variant="ghost"
                className="w-full justify-start aria-selected:bg-muted"
                aria-selected={item.category === selectedCategory}
                onClick={() => setSelectedCategory(item.category)}
              >
                {item.category}
              </Button>
            ))}
          </nav>
          <section className="min-w-0 min-h-0 p-6 pr-12 overflow-auto">
            <h2 className="mb-4 font-heading text-base font-medium">{selectedConfig?.category}</h2>
            {Content && <Content />}
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
};
