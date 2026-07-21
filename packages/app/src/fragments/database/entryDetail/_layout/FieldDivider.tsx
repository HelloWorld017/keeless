export const FieldDivider = ({ label, editing = false }: { label: string; editing?: boolean }) => (
  <div
    className={`flex items-center gap-3 text-xs text-muted-foreground ${editing ? 'py-1' : 'px-4 py-3'}`}
  >
    <span className="h-px flex-1 bg-border" />
    {label && <span>{label}</span>}
    <span className="h-px flex-1 bg-border" />
  </div>
);
